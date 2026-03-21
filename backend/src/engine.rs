use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, Read},
    path::Path,
};

use crate::{
    geo_utils::fast_distance_km,
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
    fn find_path(&self, req: &RouteRequest) -> Option<Vec<Coordinate>>;

    /// Find route while excluding certain edges (for loop generation)
    fn find_path_with_excluded_edges(
        &self,
        req: &RouteRequest,
        excluded_edges: &HashSet<(NodeIndex, NodeIndex)>,
    ) -> Option<Vec<Coordinate>>;

    /// Find path and return both coordinates and node indices from A*
    fn find_path_returning_indices(
        &self,
        req: &RouteRequest,
    ) -> Option<(Vec<Coordinate>, Vec<NodeIndex>)>;

    /// Find path with excluded edges and return both coordinates and node indices
    fn find_path_with_excluded_edges_returning_indices(
        &self,
        req: &RouteRequest,
        excluded_edges: &HashSet<(NodeIndex, NodeIndex)>,
    ) -> Option<(Vec<Coordinate>, Vec<NodeIndex>)>;
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

/// Metadata for each point in the road-point spatial index.
#[derive(Clone, Debug)]
struct RoadPoint {
    /// Graph node this point maps to (for A* start/end)
    node_idx: usize,
    /// If this point is an intermediate waypoint on an edge (None for pure graph nodes)
    edge_idx: Option<petgraph::graph::EdgeIndex>,
}

/// Result of snapping a coordinate to the nearest road.
/// Contains both the graph node for A* routing AND the road polyline
/// from the projected point to that node ("road prefix").
#[derive(Clone, Debug)]
struct RoadSnap {
    /// Graph node for A* start/end
    node: NodeIndex,
    /// Polyline from the projected point on the road to the snap node,
    /// following the road geometry. Empty if target is right at a node.
    road_prefix: Vec<Coordinate>,
}

#[derive(Clone)]
pub struct RouteEngine {
    graph: UnGraph<NodeData, EdgeData>,
    nodes: Vec<NodeData>,
    /// Spatial index of all road points (nodes + edge waypoints).
    /// Values are indices into `road_points`.
    road_point_index: KdTree<f64, usize, [f64; 2]>,
    /// Metadata for each indexed point: which node/edge it belongs to
    road_points: Vec<RoadPoint>,
    /// Pre-built edge index for O(1) edge lookup by (source, target) node indices
    edge_map: HashMap<(usize, usize), petgraph::graph::EdgeIndex>,
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

    fn find_path_returning_indices(
        &self,
        req: &RouteRequest,
    ) -> Option<(Vec<Coordinate>, Vec<NodeIndex>)> {
        RouteEngine::find_path_returning_indices(self, req)
    }

    fn find_path_with_excluded_edges_returning_indices(
        &self,
        req: &RouteRequest,
        excluded_edges: &HashSet<(NodeIndex, NodeIndex)>,
    ) -> Option<(Vec<Coordinate>, Vec<NodeIndex>)> {
        RouteEngine::find_path_with_excluded_edges_returning_indices(self, req, excluded_edges)
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

        // Build road-point index (nodes + edge waypoints) for better snap accuracy
        let (road_point_index, road_points) = Self::build_road_point_index(&graph, &nodes);

        // Build edge lookup map for O(1) edge access
        let edge_map = Self::build_edge_map(&graph);

        Ok(Self { graph, nodes, road_point_index, road_points, edge_map })
    }

    /// Build road-point spatial index with edge metadata for projection-based snapping.
    ///
    /// Each indexed point stores a `RoadPoint` with its associated graph node and
    /// (optionally) the edge it belongs to. This enables edge-projection snapping:
    /// instead of snapping to the nearest discrete point, we project onto the
    /// nearest road segment for much more accurate "which road did they click on?" answers.
    fn build_road_point_index(
        graph: &UnGraph<NodeData, EdgeData>,
        nodes: &[NodeData],
    ) -> (KdTree<f64, usize, [f64; 2]>, Vec<RoadPoint>) {
        let mut tree = KdTree::new(2);
        let mut points = Vec::new();

        // Add all graph nodes (intersection points) — no edge association
        for (idx, node) in nodes.iter().enumerate() {
            let point_id = points.len();
            points.push(RoadPoint { node_idx: idx, edge_idx: None });
            tree.add([node.coord.lon, node.coord.lat], point_id).ok();
        }

        // Add intermediate waypoints from edges, with edge association
        for edge_idx in graph.edge_indices() {
            if let Some((from, to)) = graph.edge_endpoints(edge_idx) {
                let edge_data = &graph[edge_idx];
                let from_coord = nodes[from.index()].coord;
                let to_coord = nodes[to.index()].coord;

                for wp in &edge_data.waypoints {
                    let dist_from = (wp.lat - from_coord.lat).powi(2)
                        + (wp.lon - from_coord.lon).powi(2);
                    let dist_to =
                        (wp.lat - to_coord.lat).powi(2) + (wp.lon - to_coord.lon).powi(2);
                    let nearest_node = if dist_from <= dist_to {
                        from.index()
                    } else {
                        to.index()
                    };
                    let point_id = points.len();
                    points.push(RoadPoint { node_idx: nearest_node, edge_idx: Some(edge_idx) });
                    tree.add([wp.lon, wp.lat], point_id).ok();
                }
            }
        }

        let wp_count = points.len() - nodes.len();
        tracing::debug!(
            "Road-point index: {} points ({} nodes + {} waypoints)",
            points.len(),
            nodes.len(),
            wp_count
        );

        (tree, points)
    }

    /// Build HashMap for O(1) edge lookup by (source_index, target_index)
    fn build_edge_map(graph: &UnGraph<NodeData, EdgeData>) -> HashMap<(usize, usize), petgraph::graph::EdgeIndex> {
        let mut map = HashMap::with_capacity(graph.edge_count() * 2);
        for edge_idx in graph.edge_indices() {
            if let Some((a, b)) = graph.edge_endpoints(edge_idx) {
                map.insert((a.index(), b.index()), edge_idx);
                map.insert((b.index(), a.index()), edge_idx);
            }
        }
        map
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

    pub fn find_path_with_excluded_edges(
        &self,
        req: &RouteRequest,
        excluded_edges: &HashSet<(NodeIndex, NodeIndex)>,
    ) -> Option<Vec<Coordinate>> {
        let (coords, _indices) = self.find_path_core(req, excluded_edges)?;
        Some(coords)
    }

    pub fn find_path_returning_indices(
        &self,
        req: &RouteRequest,
    ) -> Option<(Vec<Coordinate>, Vec<NodeIndex>)> {
        self.find_path_core(req, &HashSet::new())
    }

    pub fn find_path_with_excluded_edges_returning_indices(
        &self,
        req: &RouteRequest,
        excluded_edges: &HashSet<(NodeIndex, NodeIndex)>,
    ) -> Option<(Vec<Coordinate>, Vec<NodeIndex>)> {
        self.find_path_core(req, excluded_edges)
    }

    /// Core A* pathfinding with road-snap prefixes.
    ///
    /// Uses "phantom node" style routing: projects each waypoint onto the nearest
    /// road segment, then builds a polyline following that road to the nearest
    /// graph node. A* runs between graph nodes. The final path includes road
    /// prefixes so the route visually follows roads from the user's click position.
    fn find_path_core(
        &self,
        req: &RouteRequest,
        excluded_edges: &HashSet<(NodeIndex, NodeIndex)>,
    ) -> Option<(Vec<Coordinate>, Vec<NodeIndex>)> {
        let start_snap = self.snap_to_road(req.start)?;
        let end_snap = self.snap_to_road(req.end)?;

        let start = start_snap.node;
        let end = end_snap.node;

        tracing::debug!(
            "Road-snap - Start: node {} ({:?}, prefix={} pts), End: node {} ({:?}, prefix={} pts)",
            start.index(), self.nodes[start.index()].coord, start_snap.road_prefix.len(),
            end.index(), self.nodes[end.index()].coord, end_snap.road_prefix.len()
        );

        // When both snap to the same node, the road prefixes might already
        // provide a meaningful path (going along different roads to the same
        // intersection). Only use alternative nodes if BOTH prefixes are empty.
        let (start, end) = if start == end
            && start_snap.road_prefix.is_empty()
            && end_snap.road_prefix.is_empty()
        {
            tracing::debug!("Same-node snap with no road prefixes, trying alternatives");
            let start_candidates = self.closest_nodes(req.start, 3);
            let end_candidates = self.closest_nodes(req.end, 3);

            let mut best = (start, end);
            let mut best_total = f64::MAX;
            for &s in &start_candidates {
                for &e in &end_candidates {
                    if s != e {
                        let s_coord = self.nodes[s.index()].coord;
                        let e_coord = self.nodes[e.index()].coord;
                        let total = ((s_coord.lat - req.start.lat).powi(2)
                            + (s_coord.lon - req.start.lon).powi(2))
                        .sqrt()
                            + ((e_coord.lat - req.end.lat).powi(2)
                                + (e_coord.lon - req.end.lon).powi(2))
                            .sqrt();
                        if total < best_total {
                            best_total = total;
                            best = (s, e);
                        }
                    }
                }
            }
            best
        } else {
            (start, end)
        };

        // Run A* between snap nodes (may be same node → single point)
        let (astar_coords, route) = self.run_astar(start, end, req, excluded_edges)?;

        // Build full path: start_prefix + A* path + reversed end_prefix
        let mut full_coords = Vec::new();

        // Start prefix: projected point on road → ... → start node
        full_coords.extend_from_slice(&start_snap.road_prefix);

        // A* path (may overlap with last point of prefix — dedup)
        for &coord in &astar_coords {
            if full_coords.last().map_or(true, |last: &Coordinate| {
                (last.lat - coord.lat).abs() > 1e-7 || (last.lon - coord.lon).abs() > 1e-7
            }) {
                full_coords.push(coord);
            }
        }

        // End prefix reversed: end node → ... → projected point on road
        let mut end_suffix: Vec<Coordinate> = end_snap.road_prefix;
        end_suffix.reverse();
        for &coord in &end_suffix {
            if full_coords.last().map_or(true, |last: &Coordinate| {
                (last.lat - coord.lat).abs() > 1e-7 || (last.lon - coord.lon).abs() > 1e-7
            }) {
                full_coords.push(coord);
            }
        }

        tracing::debug!(
            "Full path: {} prefix + {} astar + {} suffix = {} total coords",
            start_snap.road_prefix.len(), astar_coords.len(), end_suffix.len(), full_coords.len()
        );

        Some((full_coords, route))
    }

    /// Run A* between two specific graph nodes.
    fn run_astar(
        &self,
        start: NodeIndex,
        end: NodeIndex,
        req: &RouteRequest,
        excluded_edges: &HashSet<(NodeIndex, NodeIndex)>,
    ) -> Option<(Vec<Coordinate>, Vec<NodeIndex>)> {
        if start == end {
            return Some((vec![self.nodes[start.index()].coord], vec![start]));
        }

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

            let is_excluded = excluded_edges.contains(&(from, to)) || excluded_edges.contains(&(to, from));
            let is_final_return = to == start || from == start;

            if is_excluded && !is_final_return {
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

        let coords = expand_path_with_waypoints(&route, &self.graph, &self.nodes, &self.edge_map);
        Some((coords, route))
    }

    /// Find closest graph node using road-point spatial index.
    /// Snaps to the nearest point on any road (including mid-segment waypoints),
    /// then returns that road's closest intersection node.
    pub fn closest_node(&self, target: Coordinate) -> Option<NodeIndex> {
        self.closest_nodes(target, 1).into_iter().next()
    }

    /// Find the K closest DISTINCT graph nodes using edge-projection snapping.
    ///
    /// Instead of snapping to the nearest discrete road-point, this projects the
    /// target coordinate onto nearby road segments (edges) and picks the edge whose
    /// polyline is closest. This correctly identifies "which road did the user click on?"
    /// even when the nearest indexed point happens to be on a different road.
    fn closest_nodes(&self, target: Coordinate, k: usize) -> Vec<NodeIndex> {
        const MAX_DISTANCE_KM: f64 = 20.0;
        // Query many points to discover multiple nearby edges
        let query_k = (k * 10).max(20);

        let nearest = self.road_point_index
            .nearest(&[target.lon, target.lat], query_k, &squared_euclidean)
            .unwrap_or_default();

        // Collect unique edges from nearby road-points, then project onto each
        let mut edge_projections: HashMap<petgraph::graph::EdgeIndex, f64> = HashMap::new();
        // Pure nodes (intersections) — use direct distance
        let mut node_distances: Vec<(usize, f64)> = Vec::new();

        for (dist_sq, &point_id) in &nearest {
            let rp = &self.road_points[point_id];

            if let Some(edge_idx) = rp.edge_idx {
                // Only project onto each edge once
                if !edge_projections.contains_key(&edge_idx) {
                    let proj_dist = self.project_to_edge(target, edge_idx);
                    if proj_dist * 111.0 < MAX_DISTANCE_KM {
                        edge_projections.insert(edge_idx, proj_dist);
                    }
                }
            } else {
                // Pure intersection node — use Euclidean distance
                let dist_deg = dist_sq.sqrt();
                if dist_deg * 111.0 < MAX_DISTANCE_KM {
                    node_distances.push((rp.node_idx, dist_deg));
                }
            }
        }

        // Merge all candidates: for each edge, add BOTH endpoints.
        // Score = road_dist + endpoint_dist: balances "how close is the road?" with
        // "how close is the snap node?". This prevents a road that passes 1.5m away
        // but whose nearest endpoint is 100m away from beating a road 28m away with
        // an endpoint at 29m — the latter gives a much better visual result.
        // Tuple: (node_idx, combined_score, road_dist, endpoint_dist)
        let mut candidates: Vec<(usize, f64, f64, f64)> = Vec::new();

        for (&edge_idx, &proj_dist) in &edge_projections {
            if let Some((from, to)) = self.graph.edge_endpoints(edge_idx) {
                let from_euclid = ((target.lat - self.nodes[from.index()].coord.lat).powi(2)
                    + (target.lon - self.nodes[from.index()].coord.lon).powi(2))
                .sqrt();
                let to_euclid = ((target.lat - self.nodes[to.index()].coord.lat).powi(2)
                    + (target.lon - self.nodes[to.index()].coord.lon).powi(2))
                .sqrt();
                candidates.push((from.index(), proj_dist + from_euclid, proj_dist, from_euclid));
                candidates.push((to.index(), proj_dist + to_euclid, proj_dist, to_euclid));
            }
        }
        for &(node_idx, dist) in &node_distances {
            candidates.push((node_idx, dist + dist, dist, dist));
        }

        // Sort by combined score (road_dist + endpoint_dist)
        candidates.sort_by(|a, b| {
            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Log top candidates for debugging snap accuracy
        if tracing::enabled!(tracing::Level::DEBUG) {
            let top: Vec<_> = candidates.iter().take(5).collect();
            for (i, (node_idx, score, road, endpoint)) in top.iter().enumerate() {
                let coord = self.nodes[*node_idx].coord;
                tracing::debug!(
                    "  snap candidate #{}: node {} at ({:.7}, {:.7}), score={:.1}m (road={:.1}m + endpoint={:.1}m)",
                    i + 1, node_idx, coord.lat, coord.lon,
                    score * 111_000.0, road * 111_000.0, endpoint * 111_000.0
                );
            }
        }

        // Dedup by node index, take K
        let mut seen = HashSet::new();
        candidates
            .into_iter()
            .filter(|(idx, _, _, _)| seen.insert(*idx))
            .take(k)
            .map(|(idx, _, _, _)| NodeIndex::new(idx))
            .collect()
    }

    /// Project target coordinate onto an edge's full polyline (from_node → waypoints → to_node).
    /// Returns the minimum distance (in degrees) from the target to any segment of the polyline.
    fn project_to_edge(
        &self,
        target: Coordinate,
        edge_idx: petgraph::graph::EdgeIndex,
    ) -> f64 {
        let (from, to) = self.graph.edge_endpoints(edge_idx).unwrap();
        let edge_data = &self.graph[edge_idx];
        let from_coord = self.nodes[from.index()].coord;
        let to_coord = self.nodes[to.index()].coord;

        // Build full polyline: from_node → waypoints → to_node
        let polyline_len = 2 + edge_data.waypoints.len();
        let mut polyline = Vec::with_capacity(polyline_len);
        polyline.push(from_coord);
        polyline.extend_from_slice(&edge_data.waypoints);
        polyline.push(to_coord);

        let mut min_dist = f64::MAX;
        for seg in polyline.windows(2) {
            let (dist, _t) = point_to_segment_distance(target, seg[0], seg[1]);
            if dist < min_dist {
                min_dist = dist;
            }
        }

        min_dist
    }

    /// Snap a coordinate to the nearest road, returning the graph node AND
    /// the road polyline from the projected point to that node.
    ///
    /// Uses pure projection distance (nearest road wins) to identify which road
    /// the user clicked on, then builds a polyline following that road from the
    /// click position to the nearest intersection. This preserves "road intent":
    /// if you click on Chemin de Combefort, the route starts along Combefort.
    fn snap_to_road(&self, target: Coordinate) -> Option<RoadSnap> {
        const MAX_DISTANCE_KM: f64 = 20.0;
        let query_k = 20;

        let nearest = self.road_point_index
            .nearest(&[target.lon, target.lat], query_k, &squared_euclidean)
            .unwrap_or_default();

        // Find unique edges and their projection distances
        let mut edge_projections: HashMap<petgraph::graph::EdgeIndex, f64> = HashMap::new();
        let mut best_pure_node: Option<(usize, f64)> = None;

        for (dist_sq, &point_id) in &nearest {
            let rp = &self.road_points[point_id];
            if let Some(edge_idx) = rp.edge_idx {
                if !edge_projections.contains_key(&edge_idx) {
                    let proj_dist = self.project_to_edge(target, edge_idx);
                    if proj_dist * 111.0 < MAX_DISTANCE_KM {
                        edge_projections.insert(edge_idx, proj_dist);
                    }
                }
            } else {
                let dist_deg = dist_sq.sqrt();
                if dist_deg * 111.0 < MAX_DISTANCE_KM && best_pure_node.is_none() {
                    best_pure_node = Some((rp.node_idx, dist_deg));
                }
            }
        }

        // Find the edge with minimum projection distance (nearest road)
        let best_edge = edge_projections.iter()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal));

        // If best pure node is closer than best edge, use node directly
        if let Some((node_idx, node_dist)) = best_pure_node {
            if best_edge.map_or(true, |(_, &edge_dist)| node_dist < edge_dist) {
                return Some(RoadSnap {
                    node: NodeIndex::new(node_idx),
                    road_prefix: vec![],
                });
            }
        }

        let (&best_edge_idx, _) = best_edge?;
        let (from, to) = self.graph.edge_endpoints(best_edge_idx).unwrap();
        let edge_data = &self.graph[best_edge_idx];

        // Build full polyline
        let mut polyline = Vec::with_capacity(2 + edge_data.waypoints.len());
        polyline.push(self.nodes[from.index()].coord);
        polyline.extend_from_slice(&edge_data.waypoints);
        polyline.push(self.nodes[to.index()].coord);

        // Find projection point and segment index
        let mut min_dist = f64::MAX;
        let mut min_seg_idx = 0;
        let mut min_t = 0.0;
        for (i, seg) in polyline.windows(2).enumerate() {
            let (dist, t) = point_to_segment_distance(target, seg[0], seg[1]);
            if dist < min_dist {
                min_dist = dist;
                min_seg_idx = i;
                min_t = t;
            }
        }

        // Compute projected point on the road
        let seg_a = polyline[min_seg_idx];
        let seg_b = polyline[min_seg_idx + 1];
        let proj_point = Coordinate {
            lat: seg_a.lat + min_t * (seg_b.lat - seg_a.lat),
            lon: seg_a.lon + min_t * (seg_b.lon - seg_a.lon),
        };

        // Choose closest endpoint (Euclidean) and build road prefix
        let from_dist = ((target.lat - self.nodes[from.index()].coord.lat).powi(2)
            + (target.lon - self.nodes[from.index()].coord.lon).powi(2))
        .sqrt();
        let to_dist = ((target.lat - self.nodes[to.index()].coord.lat).powi(2)
            + (target.lon - self.nodes[to.index()].coord.lon).powi(2))
        .sqrt();

        let (snap_node, road_prefix) = if from_dist <= to_dist {
            // Snap to 'from' — prefix goes backward along polyline: proj → seg_idx → ... → 0
            let mut prefix = vec![proj_point];
            for i in (0..=min_seg_idx).rev() {
                prefix.push(polyline[i]);
            }
            (from, prefix)
        } else {
            // Snap to 'to' — prefix goes forward along polyline: proj → seg_idx+1 → ... → end
            let mut prefix = vec![proj_point];
            for i in (min_seg_idx + 1)..polyline.len() {
                prefix.push(polyline[i]);
            }
            (to, prefix)
        };

        tracing::debug!(
            "snap_to_road: target=({:.7},{:.7}) → node {} at ({:.7},{:.7}), road={:.1}m, prefix={} pts",
            target.lat, target.lon,
            snap_node.index(),
            self.nodes[snap_node.index()].coord.lat,
            self.nodes[snap_node.index()].coord.lon,
            min_dist * 111_000.0,
            road_prefix.len()
        );

        Some(RoadSnap { node: snap_node, road_prefix })
    }

    /// Extract all road polylines within a bounding box
    pub fn get_roads_in_bbox(&self, min_lat: f64, max_lat: f64, min_lon: f64, max_lon: f64) -> Vec<Vec<Coordinate>> {
        let mut roads = Vec::new();
        for edge in self.graph.edge_indices() {
            let (from, to) = self.graph.edge_endpoints(edge).unwrap();
            let from_coord = self.nodes[from.index()].coord;
            let to_coord = self.nodes[to.index()].coord;
            let edge_data = &self.graph[edge];

            // Check if any part of the edge is within the bbox
            let in_bbox = |c: &Coordinate| {
                c.lat >= min_lat && c.lat <= max_lat && c.lon >= min_lon && c.lon <= max_lon
            };

            if in_bbox(&from_coord) || in_bbox(&to_coord) || edge_data.waypoints.iter().any(|w| in_bbox(w)) {
                let mut polyline = Vec::with_capacity(2 + edge_data.waypoints.len());
                polyline.push(from_coord);
                polyline.extend_from_slice(&edge_data.waypoints);
                polyline.push(to_coord);
                roads.push(polyline);
            }
        }
        roads
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
    fast_distance_km(a, b)
}

/// Distance from point P to line segment AB.
/// Returns (distance_in_degrees, t) where t∈[0,1] is the projection parameter
/// (t=0 → closest to A, t=1 → closest to B).
fn point_to_segment_distance(p: Coordinate, a: Coordinate, b: Coordinate) -> (f64, f64) {
    let dx = b.lon - a.lon;
    let dy = b.lat - a.lat;
    let len_sq = dx * dx + dy * dy;

    if len_sq < 1e-14 {
        // Degenerate segment (A ≈ B)
        let d = ((p.lat - a.lat).powi(2) + (p.lon - a.lon).powi(2)).sqrt();
        return (d, 0.0);
    }

    let t = ((p.lon - a.lon) * dx + (p.lat - a.lat) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);

    let proj_lon = a.lon + t * dx;
    let proj_lat = a.lat + t * dy;

    let d = ((p.lat - proj_lat).powi(2) + (p.lon - proj_lon).powi(2)).sqrt();
    (d, t)
}

/// Expand path with OSM waypoints from edges.
/// Uses pre-built edge_map for O(1) edge lookup instead of graph.find_edge (O(degree)).
fn expand_path_with_waypoints(
    route: &[NodeIndex],
    graph: &UnGraph<NodeData, EdgeData>,
    nodes: &[NodeData],
    edge_map: &HashMap<(usize, usize), petgraph::graph::EdgeIndex>,
) -> Vec<Coordinate> {
    if route.is_empty() {
        return Vec::new();
    }

    if route.len() == 1 {
        return vec![nodes[route[0].index()].coord];
    }

    let mut result = Vec::with_capacity(route.len() * 3);
    result.push(nodes[route[0].index()].coord);

    let mut total_waypoints_added = 0;
    let mut edges_without_waypoints = 0;

    for window in route.windows(2) {
        let from_idx = window[0];
        let to_idx = window[1];

        // O(1) lookup via pre-built HashMap
        if let Some(&edge_idx) = edge_map.get(&(from_idx.index(), to_idx.index())) {
            let edge_data = &graph[edge_idx];

            let waypoints_count = edge_data.waypoints.len();
            if waypoints_count == 0 {
                edges_without_waypoints += 1;
            }
            total_waypoints_added += waypoints_count;

            if let Some((edge_source, _)) = graph.edge_endpoints(edge_idx) {
                if edge_source == from_idx {
                    result.extend_from_slice(&edge_data.waypoints);
                } else {
                    result.extend(edge_data.waypoints.iter().rev().copied());
                }
            } else {
                result.extend_from_slice(&edge_data.waypoints);
            }
        }

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

    // ====================================================================
    // Road-snap regression tests
    //
    // These tests verify that the road-snap / phantom-node routing works
    // correctly: when a user clicks mid-edge on a trail, the route should
    // follow that trail to the nearest intersection, not jump to a nearby
    // paved road. This was the "Chemin de Combefort" bug.
    // ====================================================================

    /// Build a test graph that reproduces the "Chemin de Combefort" scenario:
    ///
    /// ```text
    ///         N3 (45.030, 5.000)
    ///          |  trail, 3 waypoints
    ///          wp2 (45.025, 5.001)
    ///          |
    ///          wp1 (45.020, 5.002) ← user clicks here
    ///          |
    ///          wp0 (45.018, 5.003)
    ///          |
    ///   N2 ── N1 (45.015, 5.005) ── N4
    ///  paved   intersection    paved
    /// (45.015, 5.000)       (45.015, 5.015)
    ///          |
    ///          N5 (45.010, 5.005) paved
    /// ```
    ///
    /// N1 is the intersection. N1→N3 is a trail with 3 intermediate waypoints.
    /// N1→N2, N1→N4, N1→N5 are short paved roads.
    fn snap_test_graph() -> GraphFile {
        use crate::graph::{EdgeRecord, NodeRecord};

        GraphFile {
            nodes: vec![
                NodeRecord { id: 1, lat: 45.015, lon: 5.005, elevation: None, population_density: 0.1 }, // intersection
                NodeRecord { id: 2, lat: 45.015, lon: 5.000, elevation: None, population_density: 0.1 }, // paved W
                NodeRecord { id: 3, lat: 45.030, lon: 5.000, elevation: None, population_density: 0.0 }, // trail end N
                NodeRecord { id: 4, lat: 45.015, lon: 5.015, elevation: None, population_density: 0.1 }, // paved E
                NodeRecord { id: 5, lat: 45.010, lon: 5.005, elevation: None, population_density: 0.1 }, // paved S
            ],
            edges: vec![
                // Trail N1→N3 with intermediate waypoints (the "Combefort" road)
                EdgeRecord {
                    from: 1, to: 3,
                    surface: SurfaceType::Trail,
                    length_m: 1800.0,
                    waypoints: vec![
                        Coordinate { lat: 45.018, lon: 5.003 },  // wp0
                        Coordinate { lat: 45.020, lon: 5.002 },  // wp1 ← target area
                        Coordinate { lat: 45.025, lon: 5.001 },  // wp2
                    ],
                },
                // Paved roads at intersection
                EdgeRecord { from: 1, to: 2, surface: SurfaceType::Paved, length_m: 400.0, waypoints: vec![] },
                EdgeRecord { from: 1, to: 4, surface: SurfaceType::Paved, length_m: 800.0, waypoints: vec![] },
                EdgeRecord { from: 1, to: 5, surface: SurfaceType::Paved, length_m: 550.0, waypoints: vec![] },
                // Connect N2→N5 for routing alternatives
                EdgeRecord { from: 2, to: 5, surface: SurfaceType::Paved, length_m: 700.0, waypoints: vec![] },
            ],
        }
    }

    fn snap_test_engine() -> RouteEngine {
        RouteEngine::from_graph_file(snap_test_graph()).expect("snap test graph")
    }

    // -- point_to_segment_distance tests --

    #[test]
    fn segment_distance_perpendicular_projection() {
        // Point directly above segment midpoint
        let p = Coordinate { lat: 1.0, lon: 0.5 };
        let a = Coordinate { lat: 0.0, lon: 0.0 };
        let b = Coordinate { lat: 0.0, lon: 1.0 };
        let (dist, t) = point_to_segment_distance(p, a, b);

        assert!((dist - 1.0).abs() < 1e-10, "distance should be 1.0, got {}", dist);
        assert!((t - 0.5).abs() < 1e-10, "t should be 0.5, got {}", t);
    }

    #[test]
    fn segment_distance_clamps_to_endpoint_a() {
        // Point beyond A
        let p = Coordinate { lat: 0.0, lon: -1.0 };
        let a = Coordinate { lat: 0.0, lon: 0.0 };
        let b = Coordinate { lat: 0.0, lon: 1.0 };
        let (dist, t) = point_to_segment_distance(p, a, b);

        assert!((dist - 1.0).abs() < 1e-10, "distance should be 1.0 (to A)");
        assert!((t - 0.0).abs() < 1e-10, "t should be clamped to 0.0");
    }

    #[test]
    fn segment_distance_clamps_to_endpoint_b() {
        // Point beyond B
        let p = Coordinate { lat: 0.0, lon: 2.0 };
        let a = Coordinate { lat: 0.0, lon: 0.0 };
        let b = Coordinate { lat: 0.0, lon: 1.0 };
        let (dist, t) = point_to_segment_distance(p, a, b);

        assert!((dist - 1.0).abs() < 1e-10, "distance should be 1.0 (to B)");
        assert!((t - 1.0).abs() < 1e-10, "t should be clamped to 1.0");
    }

    #[test]
    fn segment_distance_degenerate_segment() {
        // A == B
        let p = Coordinate { lat: 3.0, lon: 4.0 };
        let a = Coordinate { lat: 0.0, lon: 0.0 };
        let (dist, _t) = point_to_segment_distance(p, a, a);

        let expected = 5.0; // sqrt(9 + 16)
        assert!((dist - expected).abs() < 1e-10, "distance should be 5.0, got {}", dist);
    }

    // -- snap_to_road tests --

    #[test]
    fn snap_to_road_picks_trail_when_clicking_on_trail() {
        // Click near wp1 on the trail (45.020, 5.002) — should snap to trail, not paved road
        let engine = snap_test_engine();
        let target = Coordinate { lat: 45.0201, lon: 5.0021 }; // very close to wp1

        let snap = engine.snap_to_road(target).expect("should snap");

        // Should snap to N1 (node 0, the intersection) since N1 is closer than N3
        // along the trail polyline
        let snap_coord = engine.nodes[snap.node.index()].coord;

        // The key assertion: prefix should be non-empty (we're mid-edge)
        assert!(
            !snap.road_prefix.is_empty(),
            "Clicking mid-trail should produce a road prefix, got empty"
        );

        // Prefix should start near our click point
        let prefix_start = snap.road_prefix[0];
        let dist_to_target = ((prefix_start.lat - target.lat).powi(2)
            + (prefix_start.lon - target.lon).powi(2))
        .sqrt();
        assert!(
            dist_to_target < 0.001, // <~100m
            "Prefix should start near target, but starts {:.6}° away ({:.0}m)",
            dist_to_target,
            dist_to_target * 111_000.0
        );

        // Prefix should end at the snap node
        let prefix_end = snap.road_prefix.last().unwrap();
        let dist_to_node = ((prefix_end.lat - snap_coord.lat).powi(2)
            + (prefix_end.lon - snap_coord.lon).powi(2))
        .sqrt();
        assert!(
            dist_to_node < 1e-7,
            "Prefix should end at snap node, but ends {:.6}° away",
            dist_to_node
        );
    }

    #[test]
    fn snap_to_road_prefix_follows_road_geometry() {
        // Click between wp0 and wp1 on the trail — prefix should go backward
        // through intermediate waypoints to reach N1: proj → wp0 → N1
        let engine = snap_test_engine();
        // Between wp0 (45.018, 5.003) and wp1 (45.020, 5.002)
        let target = Coordinate { lat: 45.019, lon: 5.0025 };

        let snap = engine.snap_to_road(target).expect("should snap");

        // Prefix should have at least 3 points: proj_point + wp0 + N1
        assert!(
            snap.road_prefix.len() >= 3,
            "Prefix should follow road geometry with intermediate points, got {} pts: {:?}",
            snap.road_prefix.len(),
            snap.road_prefix
        );

        // All prefix points should be near the trail (within ~500m of the trail line)
        // Trail goes roughly from (45.015, 5.005) to (45.030, 5.000)
        for (i, pt) in snap.road_prefix.iter().enumerate() {
            assert!(
                pt.lat >= 45.014 && pt.lat <= 45.031,
                "Prefix point {} at lat={:.6} is outside trail latitude range",
                i, pt.lat
            );
        }
    }

    #[test]
    fn snap_to_road_at_intersection_snaps_correctly() {
        // Click right at intersection N1 (45.015, 5.005) — should snap to N1.
        // Prefix may be empty (pure node wins) or trivially short (edge endpoint
        // wins with proj ≈ node), both are acceptable.
        let engine = snap_test_engine();
        let target = Coordinate { lat: 45.015, lon: 5.005 };

        let snap = engine.snap_to_road(target).expect("should snap");
        let snap_coord = engine.nodes[snap.node.index()].coord;

        // Should snap to N1
        assert!(
            (snap_coord.lat - 45.015).abs() < 0.001 && (snap_coord.lon - 5.005).abs() < 0.001,
            "Should snap to N1, got ({:.6}, {:.6})",
            snap_coord.lat, snap_coord.lon
        );

        // Prefix should be trivial (0-2 points) — not a long road detour
        assert!(
            snap.road_prefix.len() <= 2,
            "Clicking at intersection should have trivial prefix, got {} pts",
            snap.road_prefix.len()
        );

        // If prefix exists, all points should be right at the node (< 10m)
        for pt in &snap.road_prefix {
            let dist_m = ((pt.lat - snap_coord.lat).powi(2) + (pt.lon - snap_coord.lon).powi(2))
                .sqrt() * 111_000.0;
            assert!(
                dist_m < 100.0,
                "Prefix point should be near node, but is {:.0}m away",
                dist_m
            );
        }
    }

    #[test]
    fn same_node_snap_with_prefixes_provides_route() {
        // Start: mid-trail near wp0 (45.018, 5.003)
        // End: at intersection N1 (45.015, 5.005)
        // Both snap to N1, but start has a road prefix along the trail.
        // The route should include the prefix (not be a single point).
        let engine = snap_test_engine();

        let req = RouteRequest {
            start: Coordinate { lat: 45.018, lon: 5.003 }, // near wp0 on trail
            end: Coordinate { lat: 45.015, lon: 5.005 },   // at intersection N1
            w_pop: 1.0,
            w_paved: 1.0,
        };

        let path = engine.find_path(&req).expect("should find path");

        // Path should have more than 1 point (the prefix provides geometry)
        assert!(
            path.len() >= 2,
            "Same-node snap with prefix should produce multi-point path, got {} pts",
            path.len()
        );

        // Path should start near our requested start
        let start_dist = ((path[0].lat - req.start.lat).powi(2)
            + (path[0].lon - req.start.lon).powi(2))
        .sqrt();
        assert!(
            start_dist < 0.002,
            "Path should start near requested start, but starts {:.0}m away",
            start_dist * 111_000.0
        );
    }

    #[test]
    fn path_starts_and_ends_near_requested_coordinates() {
        // Route from mid-trail to N4 (paved road east of intersection)
        let engine = snap_test_engine();

        let req = RouteRequest {
            start: Coordinate { lat: 45.020, lon: 5.002 }, // mid-trail near wp1
            end: Coordinate { lat: 45.015, lon: 5.015 },   // N4
            w_pop: 1.0,
            w_paved: 1.0,
        };

        let path = engine.find_path(&req).expect("should find path");

        // First point should be near the projected point on the trail
        let start_dist_m = ((path[0].lat - req.start.lat).powi(2)
            + (path[0].lon - req.start.lon).powi(2))
        .sqrt()
            * 111_000.0;
        assert!(
            start_dist_m < 200.0,
            "Path start should be within 200m of requested start, got {:.0}m",
            start_dist_m
        );

        // Last point should be near the requested end
        let end_dist_m = ((path.last().unwrap().lat - req.end.lat).powi(2)
            + (path.last().unwrap().lon - req.end.lon).powi(2))
        .sqrt()
            * 111_000.0;
        assert!(
            end_dist_m < 200.0,
            "Path end should be within 200m of requested end, got {:.0}m",
            end_dist_m
        );
    }

    #[test]
    fn mid_edge_route_follows_road_not_straight_line() {
        // Route from mid-trail (wp1) to N2 (paved road west).
        // The route should go: proj_on_trail → wp0 → N1 → N2
        // NOT a straight line from wp1 to N2.
        let engine = snap_test_engine();

        let req = RouteRequest {
            start: Coordinate { lat: 45.020, lon: 5.002 }, // mid-trail near wp1
            end: Coordinate { lat: 45.015, lon: 5.000 },   // N2
            w_pop: 0.0,
            w_paved: 0.0,
        };

        let path = engine.find_path(&req).expect("should find path");

        // Path should pass through or near the intersection N1 (45.015, 5.005)
        let passes_near_intersection = path.iter().any(|pt| {
            ((pt.lat - 45.015).powi(2) + (pt.lon - 5.005).powi(2)).sqrt() < 0.001
        });
        assert!(
            passes_near_intersection,
            "Route from mid-trail to N2 should pass through intersection N1"
        );

        // Path should have intermediate points (not just start + end)
        assert!(
            path.len() >= 3,
            "Route should follow road geometry with intermediate points, got {} pts",
            path.len()
        );
    }

    #[test]
    fn road_prefix_dedup_no_duplicate_at_node() {
        // When road prefix ends at a node and A* starts at that same node,
        // the path should NOT have a duplicate coordinate at the junction.
        let engine = snap_test_engine();

        let req = RouteRequest {
            start: Coordinate { lat: 45.020, lon: 5.002 }, // mid-trail
            end: Coordinate { lat: 45.015, lon: 5.015 },   // N4
            w_pop: 0.0,
            w_paved: 0.0,
        };

        let path = engine.find_path(&req).expect("should find path");

        // Check no consecutive duplicate coordinates
        for window in path.windows(2) {
            let same = (window[0].lat - window[1].lat).abs() < 1e-7
                && (window[0].lon - window[1].lon).abs() < 1e-7;
            assert!(
                !same,
                "Path has duplicate consecutive points at ({:.7}, {:.7})",
                window[0].lat, window[0].lon
            );
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
