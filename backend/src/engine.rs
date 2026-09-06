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
    /// Surface type of the edge this snap landed on (used for coloring).
    prefix_surface: SurfaceType,
    /// L'autre extrémité de l'arête sur laquelle on s'est projeté, et le
    /// morceau de route qui y mène depuis la projection.
    ///
    /// `node` est choisi sur la seule distance au point cliqué, sans savoir où
    /// va l'itinéraire. Quand A* repart par cette même arête, le préfixe est
    /// parcouru deux fois et se superpose : c'est l'antenne qu'on voit sur la
    /// carte. Garder l'autre bout permet de repartir dans le bon sens.
    other_node: Option<NodeIndex>,
    other_prefix: Vec<Coordinate>,
    /// Arête sur laquelle le point s'est projeté, sa géométrie complète
    /// (`from` → `to`), et la position de la projection le long de celle-ci,
    /// exprimée en « index de sommet + fraction du segment ».
    edge: Option<petgraph::graph::EdgeIndex>,
    polyline: Vec<Coordinate>,
    along: f64,
}

/// Point de la polyligne à la position `along` (index de sommet + fraction).
fn point_along(polyline: &[Coordinate], along: f64) -> Coordinate {
    let i = (along.floor().max(0.0) as usize).min(polyline.len().saturating_sub(2));
    let t = along - i as f64;

    Coordinate {
        lat: polyline[i].lat + t * (polyline[i + 1].lat - polyline[i].lat),
        lon: polyline[i].lon + t * (polyline[i + 1].lon - polyline[i].lon),
    }
}

/// Les deux extrémités tombent sur la même arête : l'itinéraire est le morceau
/// de route entre les deux projections.
///
/// Sans ce cas, A* doit passer par un nœud du graphe, et comme chaque bout se
/// raccroche à l'extrémité la plus proche de lui, le tracé sort du chemin par
/// un bout puis par l'autre — deux crochets pour un trajet en ligne droite.
fn same_edge_path(start: &RoadSnap, end: &RoadSnap) -> Option<Vec<Coordinate>> {
    if start.edge? != end.edge? {
        return None;
    }

    let polyline = &start.polyline;
    if polyline.len() < 2 {
        return None;
    }

    // Uniquement quand les deux projections tombent *à l'intérieur* de l'arête.
    // Si l'une est sur un nœud, le trajet direct n'est plus forcément le
    // meilleur — A* peut vouloir contourner, par exemple pour éviter du goudron
    // — et ce raccourci lui retirerait le choix.
    const EDGE_END: f64 = 1e-9;
    let last = (polyline.len() - 1) as f64;
    let inside = |along: f64| along > EDGE_END && along < last - EDGE_END;
    if !inside(start.along) || !inside(end.along) {
        return None;
    }

    let forward = end.along >= start.along;
    let (lo, hi) = if forward {
        (start.along, end.along)
    } else {
        (end.along, start.along)
    };

    // Sommets de la polyligne strictement compris entre les deux projections.
    let mut middle: Vec<Coordinate> = (0..polyline.len())
        .filter(|&i| (i as f64) > lo && (i as f64) < hi)
        .map(|i| polyline[i])
        .collect();
    if !forward {
        middle.reverse();
    }

    let mut path = vec![point_along(polyline, start.along)];
    path.extend(middle);
    path.push(point_along(polyline, end.along));

    Some(path)
}

/// Préfixe à coudre en tête de l'itinéraire, et nombre de nœuds à sauter.
///
/// Si le premier pas d'A* retraverse l'arête du snap, on repart de la
/// projection vers l'autre extrémité au lieu de faire l'aller-retour.
///
/// Deux nœuds suffisent : les deux bouts ne peuvent pas se corriger l'un
/// l'autre jusqu'à vider l'itinéraire, cela demanderait qu'ils partagent
/// l'arête et `same_edge_path` a déjà traité ce cas.
fn oriented_prefix<'a>(snap: &'a RoadSnap, route: &[NodeIndex]) -> (&'a [Coordinate], usize) {
    match snap.other_node {
        Some(other) if route.len() >= 2 && route[1] == other => (&snap.other_prefix, 1),
        _ => (&snap.road_prefix, 0),
    }
}

/// Pendant d'`oriented_prefix` pour la fin de l'itinéraire : si le dernier pas
/// d'A* arrive par l'arête du snap, on s'arrête à l'autre extrémité et on
/// rejoint la projection de là.
fn oriented_suffix<'a>(snap: &'a RoadSnap, route: &[NodeIndex]) -> (&'a [Coordinate], usize) {
    match snap.other_node {
        Some(other) if route.len() >= 2 && route[route.len() - 2] == other => {
            (&snap.other_prefix, 1)
        }
        _ => (&snap.road_prefix, 0),
    }
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
    /// Part du tronçon en forêt, dans `0.0..=1.0`.
    forest_ratio: f64,
    /// Intermediate waypoints for this edge (OSM geometry)
    waypoints: Vec<Coordinate>,
}

#[derive(Clone, Copy)]
pub struct WeightConfig {
    pub population: f64,
    pub paved: f64,
    /// Pénalité appliquée à ce qui n'est *pas* en forêt. Voir `edge_cost` pour
    /// la raison de ce sens de lecture.
    pub forest: f64,
}

/// Breakdown of an existing polyline scored with `RouteEngine::score_polyline`.
#[derive(Clone, Debug, Default)]
pub struct PolylineScore {
    pub length_km: f64,
    /// Weighted cost, directly comparable with a routed alternative's.
    pub cost: f64,
    pub paved_km: f64,
    pub trail_km: f64,
    pub dirt_km: f64,
    /// Length that snapped to no edge (off-graph: unmapped paths, ferries…).
    pub unmatched_km: f64,
    /// Longueur pondérée par la part boisée des tronçons empruntés.
    pub forest_km: f64,
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
                forest_ratio: edge.forest_ratio,
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

    /// Like find_path but also returns per-point paved flags (true = paved).
    pub fn find_path_with_surfaces(&self, req: &RouteRequest) -> Option<(Vec<Coordinate>, Vec<bool>)> {
        let start_snap = self.snap_to_road(req.start)?;
        let end_snap = self.snap_to_road(req.end)?;
        let start = start_snap.node;
        let end = end_snap.node;

        let (start, end) = if start == end
            && start_snap.road_prefix.is_empty()
            && end_snap.road_prefix.is_empty()
        {
            let start_candidates = self.closest_nodes(req.start, 3);
            let end_candidates = self.closest_nodes(req.end, 3);
            let mut best = (start, end);
            let mut best_total = f64::MAX;
            for &s in &start_candidates {
                for &e in &end_candidates {
                    if s != e {
                        let s_coord = self.nodes[s.index()].coord;
                        let e_coord = self.nodes[e.index()].coord;
                        let total = ((s_coord.lat - req.start.lat).powi(2) + (s_coord.lon - req.start.lon).powi(2)).sqrt()
                            + ((e_coord.lat - req.end.lat).powi(2) + (e_coord.lon - req.end.lon).powi(2)).sqrt();
                        if total < best_total { best_total = total; best = (s, e); }
                    }
                }
            }
            best
        } else {
            (start, end)
        };

        let (astar_coords, astar_surfaces, start_prefix, end_prefix) = {
            let excluded = HashSet::new();
            let (_, route) = self.run_astar(start, end, req, &excluded)?;
            let (start_prefix, skip_head) = oriented_prefix(&start_snap, &route);
            let (end_prefix, skip_tail) = oriented_suffix(&end_snap, &route);
            let core = &route[skip_head..route.len() - skip_tail];
            let (coords, surfaces) = expand_path_with_waypoints_and_surfaces(
                core,
                &self.graph,
                &self.nodes,
                &self.edge_map,
            );
            (coords, surfaces, start_prefix.to_vec(), end_prefix.to_vec())
        };

        let start_paved = matches!(start_snap.prefix_surface, SurfaceType::Paved);
        let end_paved = matches!(end_snap.prefix_surface, SurfaceType::Paved);

        if let Some(direct) = same_edge_path(&start_snap, &end_snap) {
            let surfaces = vec![start_paved; direct.len()];
            return Some((direct, surfaces));
        }

        let mut full_coords: Vec<Coordinate> = Vec::new();
        let mut full_surfaces: Vec<bool> = Vec::new();

        let push = |coords: &mut Vec<Coordinate>, surfs: &mut Vec<bool>, c: Coordinate, s: bool| {
            if coords.last().map_or(true, |last: &Coordinate| {
                (last.lat - c.lat).abs() > 1e-7 || (last.lon - c.lon).abs() > 1e-7
            }) {
                coords.push(c);
                surfs.push(s);
            }
        };

        for &c in &start_prefix {
            push(&mut full_coords, &mut full_surfaces, c, start_paved);
        }
        for (&c, &s) in astar_coords.iter().zip(astar_surfaces.iter()) {
            push(&mut full_coords, &mut full_surfaces, c, s);
        }
        let mut end_suffix = end_prefix;
        end_suffix.reverse();
        for &c in &end_suffix {
            push(&mut full_coords, &mut full_surfaces, c, end_paved);
        }

        Some((full_coords, full_surfaces))
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

        // Aucun nœud du graphe n'est traversé, d'où la liste d'indices vide.
        if let Some(direct) = same_edge_path(&start_snap, &end_snap) {
            return Some((direct, Vec::new()));
        }

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
        let (_, route) = self.run_astar(start, end, req, excluded_edges)?;

        // Le préfixe dépend de la direction qu'a prise A*, d'où ce recalcul de
        // la géométrie sur la tranche utile de l'itinéraire.
        let (start_prefix, skip_head) = oriented_prefix(&start_snap, &route);
        let (end_prefix, skip_tail) = oriented_suffix(&end_snap, &route);
        let astar_coords = expand_path_with_waypoints(
            &route[skip_head..route.len() - skip_tail],
            &self.graph,
            &self.nodes,
            &self.edge_map,
        );

        // Build full path: start_prefix + A* path + reversed end_prefix
        let mut full_coords = Vec::new();

        // Start prefix: projected point on road → ... → start node
        full_coords.extend_from_slice(start_prefix);

        // A* path (may overlap with last point of prefix — dedup)
        for &coord in &astar_coords {
            if full_coords.last().map_or(true, |last: &Coordinate| {
                (last.lat - coord.lat).abs() > 1e-7 || (last.lon - coord.lon).abs() > 1e-7
            }) {
                full_coords.push(coord);
            }
        }

        // End prefix reversed: end node → ... → projected point on road
        let mut end_suffix: Vec<Coordinate> = end_prefix.to_vec();
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
            forest: req.w_forest,
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

                // Seuls les points de géométrie intermédiaires portent un
                // `edge_idx` dans l'index : une arête droite n'y figure que par
                // ses deux extrémités, son milieu n'est donc jamais « proche ».
                // Sans les arêtes incidentes aux nœuds voisins, un clic le long
                // d'une telle route se raccrochait au nœud et le tracé démarrait
                // en arrière du point posé.
                for edge in self.graph.edges(NodeIndex::new(rp.node_idx)) {
                    let edge_idx = edge.id();
                    if edge_projections.contains_key(&edge_idx) {
                        continue;
                    }
                    let proj_dist = self.project_to_edge(target, edge_idx);
                    if proj_dist * 111.0 < MAX_DISTANCE_KM {
                        edge_projections.insert(edge_idx, proj_dist);
                    }
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
                    prefix_surface: SurfaceType::Paved,
                    other_node: None,
                    other_prefix: vec![],
                    edge: None,
                    polyline: vec![],
                    along: 0.0,
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

        // Les deux moitiés de l'arête, depuis la projection. On garde les deux :
        // laquelle sert dépend de la direction que prendra A*, qu'on ne connaît
        // pas encore ici.
        let mut backward = vec![proj_point];
        for i in (0..=min_seg_idx).rev() {
            backward.push(polyline[i]);
        }
        let mut forward = vec![proj_point];
        for i in (min_seg_idx + 1)..polyline.len() {
            forward.push(polyline[i]);
        }

        let (snap_node, road_prefix, other_node, other_prefix) = if from_dist <= to_dist {
            (from, backward, to, forward)
        } else {
            (to, forward, from, backward)
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

        Some(RoadSnap {
            node: snap_node,
            road_prefix,
            prefix_surface: edge_data.surface,
            other_node: Some(other_node),
            other_prefix,
            edge: Some(best_edge_idx),
            along: min_seg_idx as f64 + min_t,
            polyline,
        })
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

    /// Nearest edge to a coordinate, with its distance in degrees.
    /// Same projection logic as `closest_nodes`, but keeps the edge instead of
    /// its endpoints — scoring a trace needs the surface, not the intersection.
    fn nearest_edge(&self, target: Coordinate) -> Option<(petgraph::graph::EdgeIndex, f64)> {
        let nearest = self
            .road_point_index
            .nearest(&[target.lon, target.lat], 20, &squared_euclidean)
            .ok()?;

        let mut best: Option<(petgraph::graph::EdgeIndex, f64)> = None;
        for (_, &point_id) in &nearest {
            if let Some(edge_idx) = self.road_points[point_id].edge_idx {
                if best.map(|(b, _)| b == edge_idx).unwrap_or(false) {
                    continue;
                }
                let dist = self.project_to_edge(target, edge_idx);
                if best.map_or(true, |(_, best_dist)| dist < best_dist) {
                    best = Some((edge_idx, dist));
                }
            }
        }
        best
    }

    /// Cost of an existing polyline under the same weights the router uses.
    ///
    /// The router only ever scores paths it built itself, so a hand-made trace
    /// (a GR, a GPX import) cannot be compared with a computed alternative.
    /// This walks the polyline, snaps each step onto the nearest edge and
    /// reuses `edge_cost`, which makes the two numbers comparable.
    ///
    /// A step further than `MAX_SNAP_KM` from any edge is off-graph: its length
    /// lands in `unmatched_km` and it is charged distance only, with no surface
    /// penalty invented for it.
    pub fn score_polyline(&self, path: &[Coordinate], weights: WeightConfig) -> PolylineScore {
        const MAX_SNAP_KM: f64 = 0.2;

        let mut score = PolylineScore::default();

        for pair in path.windows(2) {
            let step_km = straight_line_km(pair[0], pair[1]);
            if step_km <= 0.0 {
                continue;
            }
            score.length_km += step_km;

            let mid = Coordinate {
                lat: (pair[0].lat + pair[1].lat) / 2.0,
                lon: (pair[0].lon + pair[1].lon) / 2.0,
            };

            match self.nearest_edge(mid) {
                Some((edge_idx, dist_deg)) if dist_deg * 111.0 <= MAX_SNAP_KM => {
                    let edge = &self.graph[edge_idx];
                    match edge.surface {
                        SurfaceType::Paved => score.paved_km += step_km,
                        SurfaceType::Trail => score.trail_km += step_km,
                        SurfaceType::Dirt => score.dirt_km += step_km,
                    }
                    score.forest_km += step_km * edge.forest_ratio;
                    // Charge this step at the snapped edge's rate, per km.
                    let per_km = self.edge_cost(edge, weights) / edge.length_km.max(1e-9);
                    score.cost += step_km * per_km;
                }
                _ => {
                    score.unmatched_km += step_km;
                    score.cost += step_km;
                }
            }
        }

        score
    }

    fn edge_cost(&self, edge: &EdgeData, weights: WeightConfig) -> f64 {
        let paved_penalty = match edge.surface {
            SurfaceType::Paved => 1.0,
            SurfaceType::Trail => 0.2,
            SurfaceType::Dirt => 0.0,
        };

        // On pénalise le hors-forêt au lieu de bonifier la forêt : l'heuristique
        // de l'A* est la distance à vol d'oiseau, donc tout coût inférieur à
        // `length_km` la rendrait inadmissible et l'algorithme renverrait des
        // chemins sous-optimaux sans le signaler.
        let open_ground_penalty = 1.0 - edge.forest_ratio;

        edge.length_km
            * (1.0
                + weights.population * edge.mean_population_density
                + weights.paved * paved_penalty
                + weights.forest * open_ground_penalty)
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

/// Like expand_path_with_waypoints but also returns is_paved per coordinate.
fn expand_path_with_waypoints_and_surfaces(
    route: &[NodeIndex],
    graph: &UnGraph<NodeData, EdgeData>,
    nodes: &[NodeData],
    edge_map: &HashMap<(usize, usize), petgraph::graph::EdgeIndex>,
) -> (Vec<Coordinate>, Vec<bool>) {
    if route.is_empty() {
        return (Vec::new(), Vec::new());
    }
    if route.len() == 1 {
        return (vec![nodes[route[0].index()].coord], vec![true]);
    }

    let mut coords = Vec::with_capacity(route.len() * 3);
    let mut surfaces = Vec::with_capacity(route.len() * 3);
    coords.push(nodes[route[0].index()].coord);
    surfaces.push(true); // first node, surface determined by first edge

    for window in route.windows(2) {
        let from_idx = window[0];
        let to_idx = window[1];
        let is_paved = if let Some(&edge_idx) = edge_map.get(&(from_idx.index(), to_idx.index())) {
            matches!(graph[edge_idx].surface, SurfaceType::Paved)
        } else {
            true
        };

        if let Some(&edge_idx) = edge_map.get(&(from_idx.index(), to_idx.index())) {
            let edge_data = &graph[edge_idx];
            let waypoints = if let Some((edge_source, _)) = graph.edge_endpoints(edge_idx) {
                if edge_source == from_idx {
                    edge_data.waypoints.iter().copied().collect::<Vec<_>>()
                } else {
                    edge_data.waypoints.iter().rev().copied().collect::<Vec<_>>()
                }
            } else {
                edge_data.waypoints.to_vec()
            };
            for wp in waypoints {
                coords.push(wp);
                surfaces.push(is_paved);
            }
        }
        coords.push(nodes[to_idx.index()].coord);
        surfaces.push(is_paved);
    }

    // Backfill first node's surface from first edge
    if surfaces.len() > 1 {
        surfaces[0] = surfaces[1];
    }

    (coords, surfaces)
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
            w_forest: 0.0,
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
            w_forest: 0.0,
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
            w_forest: 0.0,
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
            w_forest: 0.0,
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
                    forest_ratio: 0.0,
                },
                // Paved roads at intersection
                EdgeRecord { from: 1, to: 2, surface: SurfaceType::Paved, length_m: 400.0, waypoints: vec![], forest_ratio: 0.0 },
                EdgeRecord { from: 1, to: 4, surface: SurfaceType::Paved, length_m: 800.0, waypoints: vec![], forest_ratio: 0.0 },
                EdgeRecord { from: 1, to: 5, surface: SurfaceType::Paved, length_m: 550.0, waypoints: vec![], forest_ratio: 0.0 },
                // Connect N2→N5 for routing alternatives
                EdgeRecord { from: 2, to: 5, surface: SurfaceType::Paved, length_m: 700.0, waypoints: vec![], forest_ratio: 0.0 },
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
            w_forest: 0.0,
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
            w_forest: 0.0,
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
            w_forest: 0.0,
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
            w_forest: 0.0,
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

    /// Deux itinéraires entre les mêmes points : un direct hors bois (800 m) et
    /// un détour entièrement boisé (1000 m). Sans critère forêt, A* prend le
    /// direct ; avec, il doit préférer le détour.
    fn forest_fixture() -> crate::graph::GraphFile {
        use crate::graph::{EdgeRecord, GraphFile, NodeRecord};

        GraphFile {
            nodes: vec![
                NodeRecord { id: 1, lat: 45.000, lon: 5.000, elevation: None, population_density: 0.0 },
                NodeRecord { id: 2, lat: 45.000, lon: 5.010, elevation: None, population_density: 0.0 },
                NodeRecord { id: 3, lat: 45.005, lon: 5.005, elevation: None, population_density: 0.0 },
            ],
            edges: vec![
                EdgeRecord { from: 1, to: 2, surface: SurfaceType::Trail, length_m: 800.0, waypoints: vec![], forest_ratio: 0.0 },
                EdgeRecord { from: 1, to: 3, surface: SurfaceType::Trail, length_m: 500.0, waypoints: vec![], forest_ratio: 1.0 },
                EdgeRecord { from: 3, to: 2, surface: SurfaceType::Trail, length_m: 500.0, waypoints: vec![], forest_ratio: 1.0 },
            ],
        }
    }

    fn forest_request(w_forest: f64) -> RouteRequest {
        RouteRequest {
            start: Coordinate { lat: 45.000, lon: 5.000 },
            end: Coordinate { lat: 45.000, lon: 5.010 },
            w_pop: 0.0,
            // Neutralisé pour isoler l'effet du couvert boisé : les trois
            // tronçons ont la même surface.
            w_paved: 0.0,
            w_forest,
        }
    }

    #[test]
    fn without_forest_weight_the_short_open_route_wins() {
        let engine = RouteEngine::from_graph_file(forest_fixture()).expect("engine");
        let path = engine.find_path(&forest_request(0.0)).expect("path");

        assert!(
            path.iter().all(|c| (c.lat - 45.005).abs() > 1e-4),
            "sans critère forêt le détour boisé ne doit pas être emprunté : {path:?}"
        );
    }

    #[test]
    fn forest_weight_takes_the_longer_wooded_route() {
        let engine = RouteEngine::from_graph_file(forest_fixture()).expect("engine");
        let path = engine.find_path(&forest_request(2.0)).expect("path");

        assert!(
            path.iter().any(|c| (c.lat - 45.005).abs() < 1e-4),
            "avec w_forest=2 le détour boisé doit l'emporter malgré ses 200 m de plus : {path:?}"
        );
    }

    #[test]
    fn forest_ratio_never_makes_an_edge_cheaper_than_its_length() {
        // L'heuristique A* est la distance à vol d'oiseau : un coût inférieur à
        // la longueur la rendrait inadmissible.
        let engine = RouteEngine::from_graph_file(forest_fixture()).expect("engine");
        let weights = WeightConfig { population: 0.0, paved: 0.0, forest: 5.0 };

        for edge in engine.graph.edge_weights() {
            assert!(
                engine.edge_cost(edge, weights) >= edge.length_km - 1e-9,
                "coût {} < longueur {} pour forest_ratio {}",
                engine.edge_cost(edge, weights),
                edge.length_km,
                edge.forest_ratio
            );
        }
    }


    /// Trois nœuds alignés d'ouest en est, deux tronçons.
    fn straight_line_graph() -> crate::graph::GraphFile {
        use crate::graph::{EdgeRecord, GraphFile, NodeRecord};

        GraphFile {
            nodes: vec![
                NodeRecord { id: 1, lat: 45.0, lon: 5.000, elevation: None, population_density: 0.0 },
                NodeRecord { id: 2, lat: 45.0, lon: 5.010, elevation: None, population_density: 0.0 },
                NodeRecord { id: 3, lat: 45.0, lon: 5.020, elevation: None, population_density: 0.0 },
                NodeRecord { id: 4, lat: 45.0, lon: 5.030, elevation: None, population_density: 0.0 },
            ],
            edges: vec![
                // Géométrie intermédiaire : sans elle le clic se raccroche
                // directement à un nœud et le préfixe de snap n'existe pas.
                EdgeRecord {
                    from: 1, to: 2, surface: SurfaceType::Trail, length_m: 790.0, forest_ratio: 0.0,
                    waypoints: vec![
                        Coordinate { lat: 45.0, lon: 5.002 },
                        Coordinate { lat: 45.0, lon: 5.004 },
                        Coordinate { lat: 45.0, lon: 5.006 },
                        Coordinate { lat: 45.0, lon: 5.008 },
                    ],
                },
                EdgeRecord {
                    from: 2, to: 3, surface: SurfaceType::Trail, length_m: 790.0, forest_ratio: 0.0,
                    waypoints: vec![
                        Coordinate { lat: 45.0, lon: 5.012 },
                        Coordinate { lat: 45.0, lon: 5.014 },
                        Coordinate { lat: 45.0, lon: 5.016 },
                    ],
                },
                EdgeRecord {
                    from: 3, to: 4, surface: SurfaceType::Trail, length_m: 790.0, forest_ratio: 0.0,
                    waypoints: vec![
                        Coordinate { lat: 45.0, lon: 5.024 },
                        Coordinate { lat: 45.0, lon: 5.027 },
                    ],
                },
            ],
        }
    }

    #[test]
    fn snapping_does_not_double_back_to_the_nearest_node() {
        let engine = RouteEngine::from_graph_file(straight_line_graph()).expect("engine");

        // Départ posé sur le tronçon 1→2, plus près de 1 ; arrivée à l'est, en 3.
        // Le point se raccroche donc au nœud 1, dans le dos de l'itinéraire.
        let start = Coordinate { lat: 45.0001, lon: 5.004 };
        let end = Coordinate { lat: 45.0, lon: 5.020 };

        let path = engine
            .find_path(&RouteRequest { start, end, w_pop: 0.0, w_paved: 0.0, w_forest: 0.0 })
            .expect("path");

        let travelled: f64 = path.windows(2).map(|p| straight_line_km(p[0], p[1])).sum();
        let direct = straight_line_km(start, end);

        assert!(
            travelled < direct * 1.2,
            "aller-retour : {travelled:.3} km parcourus pour {direct:.3} km utiles — {path:?}"
        );
    }


    #[test]
    fn a_waypoint_mid_edge_does_not_kink_the_junction() {
        // Un point d'étape posé en plein milieu d'un tronçon : chaque segment
        // est snappé de son côté, et c'est à leur jonction que l'antenne
        // apparaissait le plus souvent.
        let engine = RouteEngine::from_graph_file(straight_line_graph()).expect("engine");

        let start = Coordinate { lat: 45.0, lon: 5.001 };
        let via = Coordinate { lat: 45.0001, lon: 5.014 };
        let end = Coordinate { lat: 45.0, lon: 5.030 };

        let leg = |from, to| {
            engine
                .find_path(&RouteRequest { start: from, end: to, w_pop: 0.0, w_paved: 0.0, w_forest: 0.0 })
                .expect("leg")
        };

        let mut full = leg(start, via);
        full.extend(leg(via, end).into_iter().skip(1));

        let travelled: f64 = full.windows(2).map(|p| straight_line_km(p[0], p[1])).sum();
        let direct = straight_line_km(start, end);

        assert!(
            travelled < direct * 1.2,
            "coude à la jonction : {travelled:.3} km pour {direct:.3} km utiles — {full:?}"
        );
    }


    /// Deux tronçons rectilignes, sans aucun point de géométrie intermédiaire.
    fn bare_line_graph() -> crate::graph::GraphFile {
        use crate::graph::{EdgeRecord, GraphFile, NodeRecord};

        GraphFile {
            nodes: vec![
                NodeRecord { id: 1, lat: 45.0, lon: 5.000, elevation: None, population_density: 0.0 },
                NodeRecord { id: 2, lat: 45.0, lon: 5.010, elevation: None, population_density: 0.0 },
                NodeRecord { id: 3, lat: 45.0, lon: 5.020, elevation: None, population_density: 0.0 },
            ],
            edges: vec![
                EdgeRecord { from: 1, to: 2, surface: SurfaceType::Trail, length_m: 790.0, waypoints: vec![], forest_ratio: 0.0 },
                EdgeRecord { from: 2, to: 3, surface: SurfaceType::Trail, length_m: 790.0, waypoints: vec![], forest_ratio: 0.0 },
            ],
        }
    }

    #[test]
    fn a_click_on_a_bare_edge_starts_where_it_was_placed() {
        let engine = RouteEngine::from_graph_file(bare_line_graph()).expect("engine");

        // Posé aux trois quarts du premier tronçon, qui n'a aucun sommet
        // intermédiaire : rien ne le rendait « proche » de l'index spatial.
        let start = Coordinate { lat: 45.0, lon: 5.0075 };
        let end = Coordinate { lat: 45.0, lon: 5.020 };

        let path = engine
            .find_path(&RouteRequest { start, end, w_pop: 0.0, w_paved: 0.0, w_forest: 0.0 })
            .expect("path");

        assert!(
            (path[0].lon - start.lon).abs() < 1e-6,
            "le tracé démarre en {:.4} au lieu de {:.4} — {path:?}",
            path[0].lon,
            start.lon
        );
    }

    #[test]
    fn both_ends_on_one_edge_follow_the_road_between_them() {
        let engine = RouteEngine::from_graph_file(bare_line_graph()).expect("engine");

        // Les deux points sur le même tronçon, chacun plus près d'un bout
        // différent : sans traitement dédié, le tracé sortait par les deux.
        let start = Coordinate { lat: 45.0, lon: 5.002 };
        let end = Coordinate { lat: 45.0, lon: 5.008 };

        let path = engine
            .find_path(&RouteRequest { start, end, w_pop: 0.0, w_paved: 0.0, w_forest: 0.0 })
            .expect("path");

        let travelled: f64 = path.windows(2).map(|p| straight_line_km(p[0], p[1])).sum();
        let direct = straight_line_km(start, end);

        assert!(
            travelled < direct * 1.05,
            "crochets aux deux bouts : {travelled:.3} km pour {direct:.3} km — {path:?}"
        );
        assert!(
            path.iter().all(|c| c.lon >= 5.002 - 1e-9 && c.lon <= 5.008 + 1e-9),
            "le tracé sort du segment demandé — {path:?}"
        );
    }

}
