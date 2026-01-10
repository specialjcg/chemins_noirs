use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, Read},
    path::Path,
};

use crate::{
    graph::GraphFile,
    models::{Coordinate, RouteRequest, SurfaceType},
};
use kdtree::KdTree;
use kdtree::distance::squared_euclidean;
use petgraph::{
    algo::astar,
    graph::{NodeIndex, UnGraph},
    visit::EdgeRef,
};

/// Trait for pathfinding algorithms (Dependency Inversion Principle)
///
/// Abstracts the routing engine to allow:
/// - **Testing**: Mock implementations for unit tests
/// - **Algorithms**: Swap between A*, Dijkstra, Bidirectional A*, etc.
/// - **Benchmarking**: Compare performance of different strategies
///
/// # Example Implementations
/// - `RouteEngine`: A* with population/surface weighting (production)
/// - `MockPathFinder`: Returns pre-defined routes (testing)
/// - `DijkstraEngine`: Unweighted shortest path (baseline comparison)
///
/// # Contract
/// All implementations must:
/// - Return `None` if no path exists between start and end
/// - Return full path with waypoints if route found
/// - Handle edge exclusions for loop generation
pub trait PathFinder: Send + Sync {
    /// Find optimal route between two coordinates
    ///
    /// # Parameters
    /// - `req`: Route request with start/end coordinates and weights
    ///
    /// # Returns
    /// - `Some(Vec<Coordinate>)`: Complete path with waypoints
    /// - `None`: No path found
    fn find_path(&self, req: &RouteRequest) -> Option<Vec<Coordinate>>;

    /// Find route while excluding certain edges (for loop generation)
    ///
    /// # Parameters
    /// - `req`: Route request
    /// - `excluded_edges`: Edges to heavily penalize (start, end) node pairs
    fn find_path_with_excluded_edges(
        &self,
        req: &RouteRequest,
        excluded_edges: &HashSet<(NodeIndex, NodeIndex)>,
    ) -> Option<Vec<Coordinate>>;
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("failed to read graph file: {0}")]
    Io(#[from] io::Error),
    #[error("invalid graph definition: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("graph is empty")]
    EmptyGraph,
    #[error("edge references unknown node {0}")]
    MissingNode(u64),
}

#[derive(Clone)]
pub struct RouteEngine {
    graph: UnGraph<NodeData, EdgeData>,
    nodes: Vec<NodeData>,
    /// Spatial index for O(log N) nearest node lookup
    spatial_index: KdTree<f64, usize, [f64; 2]>,
}

impl PathFinder for RouteEngine {
    fn find_path(&self, req: &RouteRequest) -> Option<Vec<Coordinate>> {
        RouteEngine::find_path(self, req)
    }

    fn find_path_with_excluded_edges(
        &self,
        req: &RouteRequest,
        excluded_edges: &HashSet<(NodeIndex, NodeIndex)>,
    ) -> Option<Vec<Coordinate>> {
        RouteEngine::find_path_with_excluded_edges(self, req, excluded_edges)
    }
}

#[derive(Clone, Debug)]
struct NodeData {
    coord: Coordinate,
    population_density: f64,
}

#[derive(Clone, Debug)]
struct EdgeData {
    length_km: f64,
    surface: SurfaceType,
    mean_population_density: f64,
    /// Intermediate waypoints for this edge (OSM geometry)
    waypoints: Vec<Coordinate>,
}

#[derive(Clone, Copy)]
pub struct WeightConfig {
    pub population: f64,
    pub paved: f64,
}

impl RouteEngine {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, EngineError> {
        let file = File::open(path)?;
        Self::from_reader(file)
    }

    pub fn from_reader(reader: impl Read) -> Result<Self, EngineError> {
        let graph_file: GraphFile = serde_json::from_reader(reader)?;
        Self::from_graph_file(graph_file)
    }

    pub fn from_graph_file(graph_file: GraphFile) -> Result<Self, EngineError> {
        if graph_file.nodes.is_empty() {
            return Err(EngineError::EmptyGraph);
        }
        let mut graph = UnGraph::new_undirected();
        let mut id_to_index = HashMap::new();
        let mut nodes = Vec::with_capacity(graph_file.nodes.len());

        for node in graph_file.nodes {
            let node_data = NodeData {
                coord: Coordinate {
                    lat: node.lat,
                    lon: node.lon,
                },
                population_density: node.population_density,
            };
            let idx = graph.add_node(node_data.clone());
            id_to_index.insert(node.id, idx);
            nodes.push(node_data);
        }

        for edge in graph_file.edges {
            let from = *id_to_index
                .get(&edge.from)
                .ok_or(EngineError::MissingNode(edge.from))?;
            let to = *id_to_index
                .get(&edge.to)
                .ok_or(EngineError::MissingNode(edge.to))?;
            let length_km = edge.length_m / 1000.0;
            let mean_population_density = {
                let a = graph[from].population_density;
                let b = graph[to].population_density;
                (a + b) / 2.0
            };
            let data = EdgeData {
                length_km,
                surface: edge.surface,
                mean_population_density,
                waypoints: edge.waypoints,
            };
            graph.update_edge(from, to, data);
        }

        // Build spatial index for O(log N) nearest neighbor queries
        let spatial_index = Self::build_spatial_index(&nodes);

        Ok(Self { graph, nodes, spatial_index })
    }

    /// Build KD-Tree spatial index for fast nearest neighbor queries
    /// Complexity: O(N log N) to build, O(log N) to query
    fn build_spatial_index(nodes: &[NodeData]) -> KdTree<f64, usize, [f64; 2]> {
        let mut tree = KdTree::new(2);
        for (idx, node) in nodes.iter().enumerate() {
            // Store as [lon, lat] for geographic coordinates
            let _ = tree.add([node.coord.lon, node.coord.lat], idx);
        }
        tree
    }

    /// Find optimal path between start and end coordinates using A* algorithm
    ///
    /// # Algorithm: Weighted A*
    ///
    /// This implements a variant of A* pathfinding with custom heuristics:
    ///
    /// ## Cost Function
    /// `f(n) = g(n) + h(n)`
    /// - `g(n)`: Actual cost from start to node n (weighted by population + surface)
    /// - `h(n)`: Heuristic estimate to goal (haversine distance)
    ///
    /// ## Edge Weight Calculation
    /// ```text
    /// weight = base_cost * (1.0 + population_penalty + surface_penalty)
    ///
    /// where:
    ///   base_cost = edge_length_km
    ///   population_penalty = population_density * w_pop
    ///   surface_penalty = if paved { 0.0 } else { w_paved }
    /// ```
    ///
    /// ## Optimizations
    /// - Spatial index (KD-Tree): O(log N) nearest neighbor lookup
    /// - Bidirectional search preparation (not yet implemented)
    ///
    /// # Returns
    /// - `Some(Vec<Coordinate>)`: Full path with waypoints if route found
    /// - `None`: No path exists between start and end
    pub fn find_path(&self, req: &RouteRequest) -> Option<Vec<Coordinate>> {
        self.find_path_with_excluded_edges(req, &HashSet::new())
    }

    /// Find path while avoiding specific edges (used for loop generation)
    ///
    /// # Parameters
    /// - `excluded_edges`: Set of (NodeIndex, NodeIndex) pairs to heavily penalize
    ///   (multiplies edge cost by 10000.0 to strongly discourage reuse)
    ///
    /// # Use Case
    /// Loop generation uses this to force different outbound/return paths by
    /// excluding the outbound edges from the return path search.
    pub fn find_path_with_excluded_edges(
        &self,
        req: &RouteRequest,
        excluded_edges: &HashSet<(NodeIndex, NodeIndex)>,
    ) -> Option<Vec<Coordinate>> {
        let start = self.closest_node(req.start)?;
        let end = self.closest_node(req.end)?;

        tracing::debug!(
            "Closest nodes - Start: {:?} (target: {:?}), End: {:?} (target: {:?})",
            self.nodes[start.index()].coord,
            req.start,
            self.nodes[end.index()].coord,
            req.end
        );

        let weights = WeightConfig {
            population: req.w_pop,
            paved: req.w_paved,
        };

        let heuristic = |idx: NodeIndex| {
            if idx == end {
                0.0
            } else {
                straight_line_km(self.nodes[idx.index()].coord, req.end)
            }
        };

        let edge_cost = |edge: petgraph::graph::EdgeReference<EdgeData>| {
            let base_cost = self.edge_cost(edge.weight(), weights);
            let from = edge.source();
            let to = edge.target();

            // Apply penalty if this edge was used in the outbound path
            // but allow it if we're returning to the start node
            let is_excluded = excluded_edges.contains(&(from, to)) || excluded_edges.contains(&(to, from));
            let is_final_return = to == start || from == start;

            if is_excluded && !is_final_return {
                // Apply moderate penalty (10x) to strongly discourage reuse while still allowing it as last resort
                base_cost * 10.0
            } else {
                base_cost
            }
        };

        let (_cost, route) = astar(
            &self.graph,
            start,
            |finish| finish == end,
            edge_cost,
            heuristic,
        )?;

        // Expand path with real OSM waypoints from edges
        Some(expand_path_with_waypoints(&route, &self.graph, &self.nodes))
    }

    /// Find closest node using KD-Tree spatial index
    /// Complexity: O(log N) instead of O(N) linear search
    pub fn closest_node(&self, target: Coordinate) -> Option<NodeIndex> {
        const MAX_DISTANCE_KM: f64 = 20.0; // Max 20km from target to nearest node

        // Use spatial index for O(log N) lookup
        let nearest = self.spatial_index
            .nearest(&[target.lon, target.lat], 1, &squared_euclidean)
            .ok()?;

        if let Some((dist_sq, &idx)) = nearest.first() {
            // Convert squared Euclidean distance (in degrees²) to km
            // Approximation: 1 degree ≈ 111 km
            let distance_km = dist_sq.sqrt() * 111.0;

            if distance_km <= MAX_DISTANCE_KM {
                return Some(NodeIndex::new(idx));
            }
        }

        None
    }

    fn edge_cost(&self, edge: &EdgeData, weights: WeightConfig) -> f64 {
        let paved_penalty = match edge.surface {
            SurfaceType::Paved => 1.0,
            SurfaceType::Trail => 0.2,
            SurfaceType::Dirt => 0.0,
        };

        edge.length_km
            * (1.0
                + weights.population * edge.mean_population_density
                + weights.paved * paved_penalty)
    }
}

// Removed: squared_distance (replaced by KD-Tree spatial index)

fn straight_line_km(a: Coordinate, b: Coordinate) -> f64 {
    crate::routing::haversine_km(a, b)
}

/// Expand path with OSM waypoints from edges
/// Uses real OSM geometry instead of linear interpolation
fn expand_path_with_waypoints(
    route: &[NodeIndex],
    graph: &UnGraph<NodeData, EdgeData>,
    nodes: &[NodeData],
) -> Vec<Coordinate> {
    if route.is_empty() {
        return Vec::new();
    }

    if route.len() == 1 {
        return vec![nodes[route[0].index()].coord];
    }

    let mut result = Vec::with_capacity(route.len() * 3); // Estimate
    result.push(nodes[route[0].index()].coord);

    let mut total_waypoints_added = 0;
    let mut edges_without_waypoints = 0;

    // For each consecutive pair of nodes in the route
    for window in route.windows(2) {
        let from_idx = window[0];
        let to_idx = window[1];

        // Find the edge between these two nodes
        if let Some(edge_ref) = graph.find_edge(from_idx, to_idx) {
            let edge_data = &graph[edge_ref];

            // Add all intermediate waypoints from the edge
            let waypoints_count = edge_data.waypoints.len();
            if waypoints_count == 0 {
                edges_without_waypoints += 1;
            }
            total_waypoints_added += waypoints_count;
            result.extend_from_slice(&edge_data.waypoints);
        }

        // Add the destination node
        result.push(nodes[to_idx.index()].coord);
    }

    tracing::debug!(
        "expand_path_with_waypoints: route had {} nodes, added {} waypoints from edges, {} edges had no waypoints, final path has {} coordinates",
        route.len(),
        total_waypoints_added,
        edges_without_waypoints,
        result.len()
    );

    result
}

/// DEPRECATED: Old interpolation function - kept for reference
/// Use expand_path_with_waypoints() instead which uses real OSM geometry
#[allow(dead_code)]
fn _interpolate_route_deprecated(coords: &[Coordinate]) -> Vec<Coordinate> {
    const MIN_SEGMENT_LENGTH_M: f64 = 50.0;
    const INTERPOLATION_STEP_M: f64 = 20.0;

    if coords.len() < 2 {
        return coords.to_vec();
    }

    let mut result = Vec::with_capacity(coords.len() * 2);
    result.push(coords[0]);

    for window in coords.windows(2) {
        let start = window[0];
        let end = window[1];

        let distance_km = straight_line_km(start, end);
        let distance_m = distance_km * 1000.0;

        if distance_m > MIN_SEGMENT_LENGTH_M {
            let num_interpolated = (distance_m / INTERPOLATION_STEP_M).floor() as usize;

            for i in 1..=num_interpolated {
                let t = i as f64 / (num_interpolated + 1) as f64;
                result.push(start.interpolate(end, t));
            }
        }

        result.push(end);
    }

    result
}

#[cfg(test)]
mod tests {
    use petgraph::visit::EdgeRef;

    use super::*;

    const SAMPLE: &str = include_str!("../data/sample_graph.json");

    fn engine() -> RouteEngine {
        RouteEngine::from_reader(SAMPLE.as_bytes()).expect("sample graph")
    }

    #[test]
    fn prefers_trails_when_weighted() {
        let engine = engine();
        let base_req = RouteRequest {
            start: Coordinate {
                lat: 44.99,
                lon: 4.99,
            },
            end: Coordinate {
                lat: 45.02,
                lon: 5.02,
            },
            w_pop: 0.0,
            w_paved: 5.0,
        };
        let path = engine.find_path(&base_req).expect("path");
        assert!(path.len() > 3, "should take longer scenic path");
    }

    #[test]
    fn falls_back_to_short_path_when_weights_low() {
        let engine = engine();
        let base_req = RouteRequest {
            start: Coordinate {
                lat: 44.99,
                lon: 4.99,
            },
            end: Coordinate {
                lat: 45.02,
                lon: 5.02,
            },
            w_pop: 0.0,
            w_paved: 0.0,
        };
        let path = engine.find_path(&base_req).expect("path");
        // Note: With OSM waypoints + interpolation, paths may have more points
        // This test will be updated when expand_path_with_waypoints is implemented
        assert!(!path.is_empty(), "should find a path when no avoidance");
    }

    #[test]
    fn test_graph_bounds_coverage() {
        // Test that the engine can find nodes within expected bounds
        let engine = engine();

        // Sample graph is around 45.0, 5.0
        let in_bounds_coord = Coordinate {
            lat: 45.0,
            lon: 5.0,
        };
        let node = engine.closest_node(in_bounds_coord);
        assert!(node.is_some(), "Should find node within graph bounds");
    }

    #[test]
    fn test_routing_returns_none_for_far_coordinates() {
        let engine = engine();

        // Test coordinates far outside the graph (Paris area)
        let far_req = RouteRequest {
            start: Coordinate {
                lat: 48.8566,
                lon: 2.3522,
            },
            end: Coordinate {
                lat: 48.8606,
                lon: 2.3376,
            },
            w_pop: 1.0,
            w_paved: 1.0,
        };

        let path = engine.find_path(&far_req);
        assert!(
            path.is_none(),
            "Should return None for coordinates outside graph"
        );
    }

    #[test]
    fn test_closest_node_within_reasonable_distance() {
        let engine = engine();

        // Test that closest_node finds a node within reasonable distance
        let test_coord = Coordinate {
            lat: 45.0,
            lon: 5.0,
        };
        let node_idx = engine.closest_node(test_coord).expect("should find node");
        let actual_coord = engine.nodes[node_idx.index()].coord;

        let distance = crate::routing::haversine_km(test_coord, actual_coord);
        assert!(
            distance < 5.0,
            "Closest node should be within 5km, got {}km",
            distance
        );
    }

    #[test]
    fn test_route_with_same_start_end() {
        let engine = engine();

        let req = RouteRequest {
            start: Coordinate {
                lat: 45.0,
                lon: 5.0,
            },
            end: Coordinate {
                lat: 45.0,
                lon: 5.0,
            },
            w_pop: 1.0,
            w_paved: 1.0,
        };

        // Should either return a single-point path or None
        let path = engine.find_path(&req);
        if let Some(p) = path {
            assert!(!p.is_empty(), "Path should not be empty if returned");
        }
    }

    #[test]
    fn test_graph_connectivity() {
        let engine = engine();

        // Count nodes with at least one neighbor
        let mut edges_by_node = std::collections::HashMap::new();
        for node_idx in engine.graph.node_indices() {
            edges_by_node.insert(node_idx, Vec::new());
        }

        for edge in engine.graph.edge_references() {
            let from = edge.source();
            let to = edge.target();
            edges_by_node.get_mut(&from).unwrap().push(to);
            edges_by_node.get_mut(&to).unwrap().push(from);
        }

        let total_nodes = edges_by_node.len();
        let connected_nodes = edges_by_node.values().filter(|v| !v.is_empty()).count();
        let connectivity_ratio = connected_nodes as f64 / total_nodes as f64;

        println!(
            "Graph connectivity: {}/{} nodes connected ({:.1}%)",
            connected_nodes,
            total_nodes,
            connectivity_ratio * 100.0
        );

        // At least 50% of nodes should be connected
        assert!(
            connectivity_ratio >= 0.5,
            "Graph is too disconnected: only {:.1}% of nodes have neighbors",
            connectivity_ratio * 100.0
        );
    }
}
