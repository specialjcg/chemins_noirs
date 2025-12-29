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
use petgraph::{
    algo::astar,
    graph::{NodeIndex, UnGraph},
    visit::EdgeRef,
};

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

    fn from_graph_file(graph_file: GraphFile) -> Result<Self, EngineError> {
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
            };
            graph.update_edge(from, to, data);
        }

        Ok(Self { graph, nodes })
    }

    pub fn find_path(&self, req: &RouteRequest) -> Option<Vec<Coordinate>> {
        self.find_path_with_excluded_edges(req, &HashSet::new())
    }

    /// Find path while avoiding specific edges (used for loop generation)
    /// excluded_edges: set of (NodeIndex, NodeIndex) pairs representing edges to penalize heavily
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

        Some(
            route
                .into_iter()
                .map(|idx| self.nodes[idx.index()].coord)
                .collect(),
        )
    }

    pub fn closest_node(&self, target: Coordinate) -> Option<NodeIndex> {
        const MAX_DISTANCE_KM: f64 = 20.0; // Max 20km from target to nearest node

        self.nodes
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = squared_distance(a.coord, target);
                let db = squared_distance(b.coord, target);
                da.partial_cmp(&db).unwrap()
            })
            .and_then(|(idx, node)| {
                let distance_km = straight_line_km(node.coord, target);
                if distance_km <= MAX_DISTANCE_KM {
                    Some(NodeIndex::new(idx))
                } else {
                    None
                }
            })
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

fn squared_distance(a: Coordinate, b: Coordinate) -> f64 {
    let dx = a.lon - b.lon;
    let dy = a.lat - b.lat;
    dx * dx + dy * dy
}

fn straight_line_km(a: Coordinate, b: Coordinate) -> f64 {
    crate::routing::haversine_km(a, b)
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
        assert!(path.len() <= 4, "should take direct path when no avoidance");
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
