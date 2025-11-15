use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufWriter, Write},
    path::Path,
};

use osmpbf::{Element, ElementReader};
use serde::{Deserialize, Serialize};

use crate::models::{Coordinate, SurfaceType};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphFile {
    pub nodes: Vec<NodeRecord>,
    pub edges: Vec<EdgeRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeRecord {
    pub id: u64,
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub population_density: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeRecord {
    pub from: u64,
    pub to: u64,
    pub surface: SurfaceType,
    pub length_m: f64,
}

impl GraphFile {
    pub fn read_from_path(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let file = File::open(path)?;
        serde_json::from_reader(file).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    pub fn write_to_path(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        let file = File::create(path)?;
        // 8MB buffer for fast writes, compact JSON for smaller file size
        let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, file);
        serde_json::to_writer(&mut writer, self)?;
        writer.flush()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

impl BoundingBox {
    pub fn contains(&self, coord: Coordinate) -> bool {
        coord.lat >= self.min_lat
            && coord.lat <= self.max_lat
            && coord.lon >= self.min_lon
            && coord.lon <= self.max_lon
    }

    /// Create a bounding box from two points with a margin in kilometers
    pub fn from_route(start: Coordinate, end: Coordinate, margin_km: f64) -> Self {
        // Approximate: 1 degree latitude ≈ 111 km
        let lat_margin = margin_km / 111.0;
        // Longitude degree varies with latitude, use average
        let avg_lat = (start.lat + end.lat) / 2.0;
        let lon_margin = margin_km / (111.0 * avg_lat.to_radians().cos());

        Self {
            min_lat: start.lat.min(end.lat) - lat_margin,
            max_lat: start.lat.max(end.lat) + lat_margin,
            min_lon: start.lon.min(end.lon) - lon_margin,
            max_lon: start.lon.max(end.lon) + lon_margin,
        }
    }

    /// Generate a cache key hash for this bbox
    pub fn cache_key(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        // Round to 3 decimals (~100m precision) for cache hit rate
        let min_lat = (self.min_lat * 1000.0).round() as i64;
        let max_lat = (self.max_lat * 1000.0).round() as i64;
        let min_lon = (self.min_lon * 1000.0).round() as i64;
        let max_lon = (self.max_lon * 1000.0).round() as i64;

        min_lat.hash(&mut hasher);
        max_lat.hash(&mut hasher);
        min_lon.hash(&mut hasher);
        max_lon.hash(&mut hasher);

        format!("{:x}", hasher.finish())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GraphBuildError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("osm error: {0}")]
    Osm(#[from] osmpbf::Error),
    #[error("invalid graph definition: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("graph contains no nodes")]
    EmptyGraph,
}

#[derive(Default)]
pub struct GraphBuilderConfig {
    pub bbox: Option<BoundingBox>,
}

pub struct GraphBuilder {
    config: GraphBuilderConfig,
}

// Internal helper struct for node data before final ID assignment
#[derive(Debug, Clone)]
struct OsmNode {
    osm_id: i64,
    lat: f64,
    lon: f64,
}

// Internal state for node collection
#[derive(Debug, Clone)]
struct NodeCollectionState {
    nodes: Vec<NodeRecord>,
    coords: Vec<Coordinate>,
    osm_to_graph_id: HashMap<i64, u64>,
}

impl NodeCollectionState {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            coords: Vec::new(),
            osm_to_graph_id: HashMap::new(),
        }
    }

    // Pure function to add a node, returning new state
    fn with_node(mut self, osm_id: i64, lat: f64, lon: f64) -> Self {
        let graph_id = (self.nodes.len() + 1) as u64;
        let coord = Coordinate { lat, lon };

        self.nodes.push(NodeRecord {
            id: graph_id,
            lat,
            lon,
            population_density: 0.0,
        });
        self.coords.push(coord);
        self.osm_to_graph_id.insert(osm_id, graph_id);
        self
    }
}

impl GraphBuilder {
    pub fn new(config: GraphBuilderConfig) -> Self {
        Self { config }
    }

    /// Build graph from PBF with optional caching
    pub fn build_from_pbf(&self, path: impl AsRef<Path>) -> Result<GraphFile, GraphBuildError> {
        let path = path.as_ref();

        // First pass: collect nodes
        let node_state = self.collect_nodes(path)?;

        if node_state.nodes.is_empty() {
            return Err(GraphBuildError::EmptyGraph);
        }

        // Second pass: collect edges
        let edges = self.collect_edges(path, &node_state)?;

        Ok(GraphFile {
            nodes: node_state.nodes,
            edges,
        })
    }

    /// Build a partial graph from PBF for a specific route with caching
    ///
    /// This generates a small graph (KB-MB) instead of full graph (100MB+)
    /// by only extracting nodes/edges within the route bounding box + margin.
    ///
    /// # Arguments
    /// * `pbf_path` - Path to the OSM PBF file
    /// * `cache_dir` - Directory for cached partial graphs
    /// * `start` - Route start coordinate
    /// * `end` - Route end coordinate
    /// * `margin_km` - Safety margin around route (default: 5km)
    ///
    /// # Returns
    /// GraphFile containing only relevant nodes/edges for this route
    pub fn build_partial_cached(
        pbf_path: impl AsRef<Path>,
        cache_dir: impl AsRef<Path>,
        start: Coordinate,
        end: Coordinate,
        margin_km: f64,
    ) -> Result<GraphFile, GraphBuildError> {
        // Calculate bounding box for this route
        let bbox = BoundingBox::from_route(start, end, margin_km);
        let cache_key = bbox.cache_key();

        // Check cache first
        let cache_path = cache_dir.as_ref().join(format!("partial_{}.json", cache_key));
        if cache_path.exists() {
            tracing::debug!("Cache hit for bbox {:?}", bbox);
            return GraphFile::read_from_path(&cache_path)
                .map_err(GraphBuildError::Io);
        }

        tracing::info!("Cache miss, generating partial graph for bbox {:?}", bbox);

        // Build graph with bbox filter
        let config = GraphBuilderConfig {
            bbox: Some(bbox),
        };
        let builder = GraphBuilder::new(config);
        let graph = builder.build_from_pbf(pbf_path)?;

        // Cache for future requests
        std::fs::create_dir_all(cache_dir.as_ref())?;
        graph.write_to_path(&cache_path)?;

        tracing::info!(
            "Partial graph cached: {} nodes, {} edges at {:?}",
            graph.nodes.len(),
            graph.edges.len(),
            cache_path
        );

        Ok(graph)
    }

    fn collect_nodes(
        &self,
        path: &Path,
    ) -> Result<NodeCollectionState, GraphBuildError> {
        let bbox = self.config.bbox;

        // If no bbox, collect all nodes
        if bbox.is_none() {
            let reader = ElementReader::from_path(path)?;
            let osm_nodes = reader.par_map_reduce(
                |element| -> Vec<OsmNode> {
                    match element {
                        Element::Node(node) => {
                            vec![OsmNode { osm_id: node.id(), lat: node.lat(), lon: node.lon() }]
                        }
                        Element::DenseNode(node) => {
                            vec![OsmNode { osm_id: node.id(), lat: node.lat(), lon: node.lon() }]
                        }
                        _ => Vec::new(),
                    }
                },
                Vec::new,
                |mut acc, nodes| { acc.extend(nodes); acc },
            )?;

            let state = osm_nodes.into_iter().fold(
                NodeCollectionState::new(),
                |state, node| state.with_node(node.osm_id, node.lat, node.lon),
            );
            return Ok(state);
        }

        // WITH BBOX: 3-pass approach to maintain connectivity
        let bbox = bbox.unwrap();

        // PASS 1: Collect nodes IN bbox to identify which ways touch the bbox
        let reader_nodes_in_bbox = ElementReader::from_path(path)?;
        let nodes_in_bbox: std::collections::HashSet<i64> = reader_nodes_in_bbox.par_map_reduce(
            |element| -> Vec<i64> {
                match element {
                    Element::Node(node) => {
                        let coord = Coordinate { lat: node.lat(), lon: node.lon() };
                        if bbox.contains(coord) {
                            vec![node.id()]
                        } else {
                            Vec::new()
                        }
                    }
                    Element::DenseNode(node) => {
                        let coord = Coordinate { lat: node.lat(), lon: node.lon() };
                        if bbox.contains(coord) {
                            vec![node.id()]
                        } else {
                            Vec::new()
                        }
                    }
                    _ => Vec::new(),
                }
            },
            Vec::new,
            |mut acc, nodes| { acc.extend(nodes); acc },
        )?.into_iter().collect();

        // PASS 2: Find ways that have at least one node in bbox
        let reader_ways = ElementReader::from_path(path)?;
        let needed_node_ids: std::collections::HashSet<i64> = reader_ways.par_map_reduce(
            |element| -> Vec<i64> {
                if let Element::Way(way) = element {
                    let tags: Vec<(String, String)> = way
                        .tags()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect();

                    if !has_supported_highway(&tags) {
                        return Vec::new();
                    }

                    let node_refs: Vec<i64> = way.refs().collect();

                    // Check if this way has AT LEAST ONE node in bbox
                    let touches_bbox = node_refs.iter().any(|id| nodes_in_bbox.contains(id));

                    if touches_bbox {
                        // Include ALL nodes from this way to maintain connectivity
                        node_refs
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            },
            Vec::new,
            |mut acc, nodes| { acc.extend(nodes); acc },
        )?.into_iter().collect();

        // PASS 3: Collect actual node data for the needed nodes
        let reader = ElementReader::from_path(path)?;
        let osm_nodes = reader.par_map_reduce(
            |element| -> Vec<OsmNode> {
                match element {
                    Element::Node(node) => {
                        if needed_node_ids.contains(&node.id()) {
                            vec![OsmNode { osm_id: node.id(), lat: node.lat(), lon: node.lon() }]
                        } else {
                            Vec::new()
                        }
                    }
                    Element::DenseNode(node) => {
                        if needed_node_ids.contains(&node.id()) {
                            vec![OsmNode { osm_id: node.id(), lat: node.lat(), lon: node.lon() }]
                        } else {
                            Vec::new()
                        }
                    }
                    _ => Vec::new(),
                }
            },
            Vec::new,
            |mut acc, nodes| { acc.extend(nodes); acc },
        )?;

        let state = osm_nodes.into_iter().fold(
            NodeCollectionState::new(),
            |state, node| state.with_node(node.osm_id, node.lat, node.lon),
        );

        Ok(state)
    }

    fn collect_edges(
        &self,
        path: &Path,
        node_state: &NodeCollectionState,
    ) -> Result<Vec<EdgeRecord>, GraphBuildError> {
        let osm_to_graph = &node_state.osm_to_graph_id;
        let coords = &node_state.coords;
        let reader = ElementReader::from_path(path)?;

        // Parallel map-reduce to collect edges
        let edges = reader.par_map_reduce(
            |element| -> Vec<EdgeRecord> {
                if let Element::Way(way) = element {
                    process_way_element(way, coords, osm_to_graph)
                } else {
                    Vec::new()
                }
            },
            Vec::new,
            |mut acc, edges| {
                acc.extend(edges);
                acc
            },
        )?;

        Ok(edges)
    }
}

// Pure function to process a node element
fn process_node_element(
    lat: f64,
    lon: f64,
    osm_id: i64,
    bbox: Option<BoundingBox>,
) -> Vec<OsmNode> {
    let coord = Coordinate { lat, lon };

    // Apply bounding box filter
    let in_bbox = bbox.map(|b| b.contains(coord)).unwrap_or(true);

    if in_bbox {
        vec![OsmNode { osm_id, lat, lon }]
    } else {
        Vec::new()
    }
}

// Pure function to process a way element and extract edges
fn process_way_element(
    way: osmpbf::elements::Way,
    coords: &[Coordinate],
    osm_to_graph: &HashMap<i64, u64>,
) -> Vec<EdgeRecord> {
    // Collect tags (convert from &str to String)
    let tags: Vec<(String, String)> = way
        .tags()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    // Check if way has supported highway
    if !has_supported_highway(&tags) {
        return Vec::new();
    }

    // Infer surface type
    let surface = infer_surface(&tags);

    // Collect node references
    let node_refs: Vec<i64> = way.refs().collect();

    // Create edges for consecutive node pairs
    node_refs
        .windows(2)
        .filter_map(|pair| {
            create_edge_record(pair[0], pair[1], surface, coords, osm_to_graph)
        })
        .collect()
}

// Pure function to create an edge record from node pair
fn create_edge_record(
    from_osm: i64,
    to_osm: i64,
    surface: SurfaceType,
    coords: &[Coordinate],
    osm_to_graph: &HashMap<i64, u64>,
) -> Option<EdgeRecord> {
    let from = osm_to_graph.get(&from_osm)?;
    let to = osm_to_graph.get(&to_osm)?;

    let coord_a = coords.get((from - 1) as usize)?;
    let coord_b = coords.get((to - 1) as usize)?;

    let length_km = crate::routing::haversine_km(*coord_a, *coord_b);

    Some(EdgeRecord {
        from: *from,
        to: *to,
        surface,
        length_m: length_km * 1000.0,
    })
}

// Pure function to check if way has supported highway
fn has_supported_highway(tags: &[(String, String)]) -> bool {
    tags.iter()
        .find(|(k, _)| k == "highway")
        .map(|(_, v)| is_supported_highway(v))
        .unwrap_or(false)
}

// Pure function to check if highway value is supported
fn is_supported_highway(highway_value: &str) -> bool {
    matches!(
        highway_value,
        "path"
            | "footway"
            | "living_street"
            | "secondary"
            | "tertiary"
            | "residential"
            | "track"
            | "service"
            | "unclassified"
            | "primary"
    )
}

// Pure function to infer surface type from tags
fn infer_surface(tags: &[(String, String)]) -> SurfaceType {
    let tags_map: HashMap<&str, &str> = tags
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    // Check explicit surface tag first
    if let Some(surface) = tags_map.get("surface") {
        return match *surface {
            "gravel" | "fine_gravel" | "compacted" | "unpaved" => SurfaceType::Trail,
            "dirt" | "earth" | "ground" | "grass" => SurfaceType::Dirt,
            _ => SurfaceType::Paved,
        };
    }

    // Fallback to highway classification
    if let Some(highway) = tags_map.get("highway") {
        return match *highway {
            "path" | "footway" | "track" => SurfaceType::Trail,
            "service" | "residential" | "primary" | "secondary" | "tertiary" => {
                SurfaceType::Paved
            }
            _ => SurfaceType::Trail,
        };
    }

    // Default fallback
    SurfaceType::Trail
}
