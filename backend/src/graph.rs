use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader, BufWriter, Write},
    num::NonZeroUsize,
    path::Path,
    sync::RwLock,
};

use lru::LruCache;
use once_cell::sync::Lazy;
use osmpbf::{Element, ElementReader};
use serde::{Deserialize, Serialize};

use crate::models::{Coordinate, SurfaceType};

/// Type aliases for complex OSM data structures
type OsmTags = Vec<(String, String)>;
type NodeIds = Vec<i64>;
type OsmWay = (i64, NodeIds, OsmTags);
type NodeCoordMap = HashMap<i64, (f64, f64, Option<f64>)>;

/// Trait for graph caching abstraction (Dependency Inversion Principle)
///
/// This allows different caching strategies to be implemented and tested:
/// - `LruGraphCache`: In-memory LRU with RwLock (default)
/// - `RedisGraphCache`: Distributed cache for multi-instance deployments
/// - `NoOpCache`: Testing/benchmarking without caching overhead
///
/// # Example
/// ```ignore
/// struct CustomCache;
/// impl GraphCache for CustomCache {
///     fn get(&self, key: &str) -> Option<GraphFile> { /* ... */ }
///     fn put(&self, key: String, graph: GraphFile) { /* ... */ }
/// }
/// ```
pub trait GraphCache: Send + Sync {
    /// Retrieve cached graph by key (non-blocking read)
    fn get(&self, key: &str) -> Option<GraphFile>;

    /// Store graph in cache (write operation)
    fn put(&self, key: String, graph: GraphFile);
}

/// Global LRU cache for partial graphs (max 20 graphs, ~280MB with 14MB each)
///
/// Performance optimization: Uses RwLock instead of Mutex for concurrent reads
/// - Read path: RwLock::read() + peek() allows parallel cache lookups
/// - Write path: RwLock::write() + put() for exclusive cache updates
/// - Benefit: Multiple route requests can check cache simultaneously
static GRAPH_CACHE: Lazy<RwLock<LruCache<String, GraphFile>>> =
    Lazy::new(|| RwLock::new(LruCache::new(NonZeroUsize::new(20).unwrap())));

/// Default LRU-based implementation of GraphCache
pub struct LruGraphCache;

impl GraphCache for LruGraphCache {
    fn get(&self, key: &str) -> Option<GraphFile> {
        GRAPH_CACHE
            .read()
            .ok()
            .and_then(|cache| cache.peek(key).cloned())
    }

    fn put(&self, key: String, graph: GraphFile) {
        if let Ok(mut cache) = GRAPH_CACHE.write() {
            cache.put(key, graph);
        }
    }
}

/// No-op cache implementation for testing/benchmarking
///
/// Always returns cache miss, never stores anything.
/// Useful for:
/// - Performance benchmarks (measure pure algorithm performance)
/// - Integration tests (ensure correct behavior without caching)
#[cfg(test)]
pub struct NoOpCache;

#[cfg(test)]
impl GraphCache for NoOpCache {
    fn get(&self, _key: &str) -> Option<GraphFile> {
        None // Always miss
    }

    fn put(&self, _key: String, _graph: GraphFile) {
        // Discard silently
    }
}

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
    pub elevation: Option<f64>, // Elevation in meters from OSM 'ele' tag
    #[serde(default)]
    pub population_density: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeRecord {
    pub from: u64,
    pub to: u64,
    pub surface: SurfaceType,
    pub length_m: f64,
    /// Intermediate waypoints between from and to nodes (excluding from/to themselves)
    /// This preserves the actual geometry of the road
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waypoints: Vec<Coordinate>,
}

impl GraphFile {
    pub fn read_from_path(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let path = path.as_ref();

        // Try compressed format first (.zst extension)
        let compressed_path = path.with_extension("json.zst");
        if compressed_path.exists() {
            return Self::read_compressed(&compressed_path);
        }

        // Fallback to uncompressed JSON
        let file = File::open(path)?;
        serde_json::from_reader(file).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    pub fn write_to_path(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        let path = path.as_ref();

        // Write compressed format (.zst) for better performance
        let compressed_path = path.with_extension("json.zst");
        self.write_compressed(&compressed_path)?;

        // Also write uncompressed for backward compatibility (can be removed later)
        let file = File::create(path)?;
        let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, file);
        serde_json::to_writer(&mut writer, self)?;
        writer.flush()
    }

    /// Write graph with Zstandard compression (60-70% space savings)
    pub fn write_compressed(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        let file = File::create(path)?;
        let mut encoder = zstd::stream::write::Encoder::new(file, 3)?; // Level 3 = good balance
        serde_json::to_writer(&mut encoder, self)?;
        encoder.finish()?;
        Ok(())
    }

    /// Read graph with Zstandard decompression
    pub fn read_compressed(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let file = File::open(path)?;
        let decoder = zstd::stream::read::Decoder::new(file)?;
        let reader = BufReader::new(decoder);
        serde_json::from_reader(reader).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
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
    /// Maximum allowed bounding box area in km² to prevent DoS attacks
    /// ~100km × 100km = reasonable maximum for a single route request
    const MAX_BBOX_AREA_KM2: f64 = 10_000.0;

    pub fn contains(&self, coord: Coordinate) -> bool {
        coord.lat >= self.min_lat
            && coord.lat <= self.max_lat
            && coord.lon >= self.min_lon
            && coord.lon <= self.max_lon
    }

    /// Validate that the bounding box is not excessively large (DoS protection)
    pub fn validate(&self) -> Result<(), &'static str> {
        let lat_diff = self.max_lat - self.min_lat;
        let lon_diff = self.max_lon - self.min_lon;

        // Approximate area calculation (1 degree lat ≈ 111km, lon varies with latitude)
        let avg_lat = (self.min_lat + self.max_lat) / 2.0;
        let area_km2 = lat_diff * 111.0 * lon_diff * (111.0 * avg_lat.to_radians().cos());

        if area_km2 > Self::MAX_BBOX_AREA_KM2 {
            return Err("Bounding box too large (max 10,000 km²)");
        }

        if lat_diff <= 0.0 || lon_diff <= 0.0 {
            return Err("Invalid bounding box: min must be less than max");
        }

        Ok(())
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

    /// Get tiles that overlap with this bbox (for tile-based loading)
    pub fn overlapping_tiles(&self, tile_size_km: f64) -> Vec<TileId> {
        let mut tiles = Vec::new();

        // Convert km to degrees (approximate)
        let lat_step = tile_size_km / 111.0;
        let avg_lat = (self.min_lat + self.max_lat) / 2.0;
        let lon_step = tile_size_km / (111.0 * avg_lat.to_radians().cos());

        // Find all tiles overlapping this bbox
        let min_tile_x = (self.min_lon / lon_step).floor() as i32;
        let max_tile_x = (self.max_lon / lon_step).ceil() as i32;
        let min_tile_y = (self.min_lat / lat_step).floor() as i32;
        let max_tile_y = (self.max_lat / lat_step).ceil() as i32;

        for x in min_tile_x..=max_tile_x {
            for y in min_tile_y..=max_tile_y {
                tiles.push(TileId { x, y });
            }
        }

        tiles
    }
}

/// Tile identifier for 20km×20km grid
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileId {
    pub x: i32,
    pub y: i32,
}

impl TileId {
    /// Calculate tile ID from coordinates (20km tiles)
    pub fn from_coord(lat: f64, lon: f64, tile_size_km: f64) -> Self {
        let lat_step = tile_size_km / 111.0;
        let lon_step = tile_size_km / (111.0 * lat.to_radians().cos());

        Self {
            x: (lon / lon_step).floor() as i32,
            y: (lat / lat_step).floor() as i32,
        }
    }

    /// Get bounding box for this tile
    pub fn bbox(&self, tile_size_km: f64) -> BoundingBox {
        let lat_step = tile_size_km / 111.0;
        // Use average latitude for longitude calculation
        let center_lat = (self.y as f64) * lat_step + lat_step / 2.0;
        let lon_step = tile_size_km / (111.0 * center_lat.to_radians().cos());

        BoundingBox {
            min_lat: (self.y as f64) * lat_step,
            max_lat: (self.y as f64 + 1.0) * lat_step,
            min_lon: (self.x as f64) * lon_step,
            max_lon: (self.x as f64 + 1.0) * lon_step,
        }
    }

    /// Get filename for this tile
    pub fn filename(&self) -> String {
        format!("tile_{}_{}.json.zst", self.x, self.y)
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
    elevation: Option<f64>, // Elevation in meters from OSM 'ele' tag
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
    fn with_node(mut self, osm_id: i64, lat: f64, lon: f64, elevation: Option<f64>) -> Self {
        let graph_id = (self.nodes.len() + 1) as u64;
        let coord = Coordinate { lat, lon };

        self.nodes.push(NodeRecord {
            id: graph_id,
            lat,
            lon,
            elevation,
            population_density: 0.0,
        });
        self.coords.push(coord);
        self.osm_to_graph_id.insert(osm_id, graph_id);
        self
    }
}

/// Pre-filtered PBF data stored in memory for fast processing
struct FilteredPbfData {
    /// All nodes in or near the bbox: osm_id -> (lat, lon, elevation)
    nodes: NodeCoordMap,
    /// All highway ways touching the bbox: (way_id, node_refs, tags)
    ways: Vec<OsmWay>,
}

impl GraphBuilder {
    pub fn new(config: GraphBuilderConfig) -> Self {
        Self { config }
    }

    /// Build graph from PBF with optional caching
    pub fn build_from_pbf(&self, path: impl AsRef<Path>) -> Result<GraphFile, GraphBuildError> {
        let path = path.as_ref();

        // Use optimized version with OSM waypoints
        // If bbox is set, use optimized single-pass filtering
        if let Some(bbox) = self.config.bbox {
            return self.build_from_pbf_optimized(path, bbox);
        }

        // Fallback to simple 4-pass approach (for full PBF without bbox)
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

    /// Optimized graph building using single-pass pre-filtering
    /// Reads PBF once, filters to bbox, then processes in-memory
    fn build_from_pbf_optimized(
        &self,
        path: &Path,
        bbox: BoundingBox,
    ) -> Result<GraphFile, GraphBuildError> {
        tracing::info!("Using optimized single-pass PBF filtering for bbox: {:?}", bbox);

        // PASS 1: Read PBF once and collect all relevant elements in memory
        let filtered_data = self.filter_pbf_to_memory(path, bbox)?;

        tracing::info!(
            "Filtered data: {} nodes, {} ways (in-memory)",
            filtered_data.nodes.len(),
            filtered_data.ways.len()
        );

        // PASS 2: Build graph from in-memory data (no file I/O)
        self.build_from_filtered_data(filtered_data, bbox)
    }

    /// Build graph from pre-generated tiles (FAST - <10s)
    ///
    /// Load only the tiles overlapping the bbox, merge them into a single graph.
    /// Requires tiles to be pre-generated using `generate_tiles_from_pbf`.
    pub fn build_from_tiles(
        &self,
        tiles_dir: impl AsRef<Path>,
        bbox: BoundingBox,
    ) -> Result<GraphFile, GraphBuildError> {
        const TILE_SIZE_KM: f64 = 20.0;

        let tiles_dir = tiles_dir.as_ref();
        let tile_ids = bbox.overlapping_tiles(TILE_SIZE_KM);

        tracing::info!(
            "Loading {} tiles for bbox: {:?}",
            tile_ids.len(),
            bbox
        );

        if tile_ids.is_empty() {
            return Err(GraphBuildError::EmptyGraph);
        }

        // Load all tiles
        let mut all_nodes: HashMap<u64, NodeRecord> = HashMap::new();
        let mut all_edges = Vec::new();

        for tile_id in &tile_ids {
            let tile_path = tiles_dir.join(tile_id.filename());

            if !tile_path.exists() {
                tracing::warn!("Tile not found: {}, skipping", tile_path.display());
                continue;
            }

            let tile_graph = GraphFile::read_compressed(&tile_path)?;

            tracing::debug!(
                "Loaded tile {:?}: {} nodes, {} edges",
                tile_id,
                tile_graph.nodes.len(),
                tile_graph.edges.len()
            );

            // Merge nodes (avoid duplicates)
            for node in tile_graph.nodes {
                all_nodes.entry(node.id).or_insert(node);
            }

            // Merge edges
            all_edges.extend(tile_graph.edges);
        }

        // Filter nodes and edges to bbox (tiles might have overlap)
        let filtered_nodes: Vec<NodeRecord> = all_nodes
            .into_values()
            .filter(|node| {
                bbox.contains(Coordinate {
                    lat: node.lat,
                    lon: node.lon,
                })
            })
            .collect();

        let node_ids: std::collections::HashSet<u64> = filtered_nodes.iter().map(|n| n.id).collect();
        let filtered_edges: Vec<EdgeRecord> = all_edges
            .into_iter()
            .filter(|edge| node_ids.contains(&edge.from) && node_ids.contains(&edge.to))
            .collect();

        tracing::info!(
            "Merged tiles: {} nodes, {} edges",
            filtered_nodes.len(),
            filtered_edges.len()
        );

        Ok(GraphFile {
            nodes: filtered_nodes,
            edges: filtered_edges,
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

        // Check in-memory LRU cache first (fastest)
        // Use peek() for lock-free concurrent reads (doesn't update LRU order)
        if let Ok(cache) = GRAPH_CACHE.read() {
            if let Some(graph) = cache.peek(&cache_key) {
                tracing::debug!("LRU cache hit (peek) for bbox {:?}", bbox);
                return Ok(graph.clone());
            }
        }

        // Check disk cache (compressed format preferred)
        let cache_path_compressed = cache_dir
            .as_ref()
            .join(format!("partial_{}.json.zst", cache_key));
        let cache_path_uncompressed = cache_dir
            .as_ref()
            .join(format!("partial_{}.json", cache_key));

        if cache_path_compressed.exists() {
            tracing::debug!("Disk cache hit (compressed) for bbox {:?}", bbox);
            let graph = GraphFile::read_compressed(&cache_path_compressed).map_err(GraphBuildError::Io)?;

            // Populate LRU cache
            if let Ok(mut cache) = GRAPH_CACHE.write() {
                cache.put(cache_key.clone(), graph.clone());
            }

            return Ok(graph);
        } else if cache_path_uncompressed.exists() {
            tracing::debug!("Disk cache hit (uncompressed) for bbox {:?}", bbox);
            let graph = GraphFile::read_from_path(&cache_path_uncompressed).map_err(GraphBuildError::Io)?;

            // Populate LRU cache
            if let Ok(mut cache) = GRAPH_CACHE.write() {
                cache.put(cache_key.clone(), graph.clone());
            }

            return Ok(graph);
        }

        tracing::info!("Cache miss, generating partial graph for bbox {:?}", bbox);

        // Build graph with bbox filter
        let config = GraphBuilderConfig { bbox: Some(bbox) };
        let builder = GraphBuilder::new(config);
        let graph = builder.build_from_pbf(pbf_path)?;

        // Cache to disk (both formats for transition period)
        std::fs::create_dir_all(cache_dir.as_ref())?;
        graph.write_to_path(&cache_path_uncompressed)?;

        tracing::info!(
            "Partial graph cached: {} nodes, {} edges at {:?}",
            graph.nodes.len(),
            graph.edges.len(),
            cache_path_compressed
        );

        // Populate LRU cache
        if let Ok(mut cache) = GRAPH_CACHE.write() {
            cache.put(cache_key, graph.clone());
        }

        Ok(graph)
    }

    /// PASS 1: Single-pass filtering - collect all relevant nodes and ways in memory
    fn filter_pbf_to_memory(
        &self,
        path: &Path,
        bbox: BoundingBox,
    ) -> Result<FilteredPbfData, GraphBuildError> {
        use std::collections::HashSet;

        let reader = ElementReader::from_path(path)?;

        // Collect everything in a single pass
        let (nodes_in_bbox, ways_data) = reader.par_map_reduce(
            |element| -> (NodeCoordMap, Vec<OsmWay>) {
                match element {
                    Element::Node(node) => {
                        let lat = node.lat();
                        let lon = node.lon();

                        // Check if node is in bbox
                        if lat >= bbox.min_lat
                            && lat <= bbox.max_lat
                            && lon >= bbox.min_lon
                            && lon <= bbox.max_lon
                        {
                            let elevation = extract_elevation(&node.tags().collect::<Vec<_>>());
                            let mut map = HashMap::new();
                            map.insert(node.id(), (lat, lon, elevation));
                            (map, Vec::new())
                        } else {
                            (HashMap::new(), Vec::new())
                        }
                    }
                    Element::DenseNode(node) => {
                        let lat = node.lat();
                        let lon = node.lon();

                        if lat >= bbox.min_lat
                            && lat <= bbox.max_lat
                            && lon >= bbox.min_lon
                            && lon <= bbox.max_lon
                        {
                            let elevation = extract_elevation(&node.tags().collect::<Vec<_>>());
                            let mut map = HashMap::new();
                            map.insert(node.id(), (lat, lon, elevation));
                            (map, Vec::new())
                        } else {
                            (HashMap::new(), Vec::new())
                        }
                    }
                    Element::Way(way) => {
                        let tags: Vec<_> = way.tags().collect();

                        // Check if this is a highway
                        if tags.iter().any(|(k, _)| *k == "highway") {
                            let node_refs: Vec<i64> = way.refs().collect();
                            let tag_pairs: Vec<(String, String)> =
                                tags.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();

                            (HashMap::new(), vec![(way.id(), node_refs, tag_pairs)])
                        } else {
                            (HashMap::new(), Vec::new())
                        }
                    }
                    _ => (HashMap::new(), Vec::new()),
                }
            },
            || (HashMap::new(), Vec::new()),
            |(mut nodes1, mut ways1), (nodes2, ways2)| {
                nodes1.extend(nodes2);
                ways1.extend(ways2);
                (nodes1, ways1)
            },
        )?;

        // Second pass: collect nodes referenced by ways but not in bbox
        let way_node_refs: HashSet<i64> = ways_data
            .iter()
            .flat_map(|(_, refs, _)| refs.iter())
            .copied()
            .collect();

        let missing_node_ids: HashSet<i64> = way_node_refs
            .difference(&nodes_in_bbox.keys().copied().collect())
            .copied()
            .collect();

        if missing_node_ids.is_empty() {
            return Ok(FilteredPbfData {
                nodes: nodes_in_bbox,
                ways: ways_data,
            });
        }

        // Collect missing nodes
        let reader2 = ElementReader::from_path(path)?;
        let missing_nodes = reader2.par_map_reduce(
            |element| -> HashMap<i64, (f64, f64, Option<f64>)> {
                match element {
                    Element::Node(node) => {
                        if missing_node_ids.contains(&node.id()) {
                            let elevation = extract_elevation(&node.tags().collect::<Vec<_>>());
                            let mut map = HashMap::new();
                            map.insert(node.id(), (node.lat(), node.lon(), elevation));
                            map
                        } else {
                            HashMap::new()
                        }
                    }
                    Element::DenseNode(node) => {
                        if missing_node_ids.contains(&node.id()) {
                            let elevation = extract_elevation(&node.tags().collect::<Vec<_>>());
                            let mut map = HashMap::new();
                            map.insert(node.id(), (node.lat(), node.lon(), elevation));
                            map
                        } else {
                            HashMap::new()
                        }
                    }
                    _ => HashMap::new(),
                }
            },
            HashMap::new,
            |mut acc, nodes| {
                acc.extend(nodes);
                acc
            },
        )?;

        // Merge all nodes
        let mut all_nodes = nodes_in_bbox;
        all_nodes.extend(missing_nodes);

        Ok(FilteredPbfData {
            nodes: all_nodes,
            ways: ways_data,
        })
    }

    /// PASS 2: Build graph from pre-filtered in-memory data
    fn build_from_filtered_data(
        &self,
        data: FilteredPbfData,
        _bbox: BoundingBox,
    ) -> Result<GraphFile, GraphBuildError> {
        // Build node collection state
        let mut node_state = NodeCollectionState::new();

        // Sort nodes by osm_id for deterministic ordering
        let mut sorted_nodes: Vec<_> = data.nodes.into_iter().collect();
        sorted_nodes.sort_by_key(|(osm_id, _)| *osm_id);

        for (osm_id, (lat, lon, elevation)) in sorted_nodes {
            // Only add if not already present (prevent duplicates)
            if !node_state.osm_to_graph_id.contains_key(&osm_id) {
                node_state = node_state.with_node(osm_id, lat, lon, elevation);
            }
        }

        if node_state.nodes.is_empty() {
            return Err(GraphBuildError::EmptyGraph);
        }

        tracing::info!(
            "Built node collection: {} unique nodes",
            node_state.nodes.len()
        );

        // Build edges from ways WITH intermediate waypoints
        let mut edges = Vec::new();

        // First, identify all intersection nodes across all ways
        let intersections = identify_intersection_nodes(&data.ways);

        tracing::debug!(
            "Identified {} intersection nodes from {} ways",
            intersections.len(),
            data.ways.len()
        );

        // For each way, split it into segments between intersections
        let ways_count = data.ways.len();

        for (_, node_refs, tags) in &data.ways {
            if node_refs.len() < 2 {
                continue;
            }

            let surface = infer_surface(tags);

            // Find intersection indices in this way
            let mut segment_start = 0;

            for (i, &node_osm_id) in node_refs.iter().enumerate() {
                // If this node is an intersection (and not the first node)
                if i > segment_start && intersections.contains(&node_osm_id) {
                    // Create edge from segment_start to i (inclusive)
                    let segment = &node_refs[segment_start..=i];

                    if let Some(edge) = build_edge_with_waypoints(
                        segment,
                        surface,
                        &node_state.osm_to_graph_id,
                        &node_state.coords,
                    ) {
                        edges.push(edge);
                    }

                    // Start new segment from this intersection
                    segment_start = i;
                }
            }

            // Handle last segment if it wasn't closed
            if segment_start < node_refs.len() - 1 {
                let segment = &node_refs[segment_start..];

                if let Some(edge) = build_edge_with_waypoints(
                    segment,
                    surface,
                    &node_state.osm_to_graph_id,
                    &node_state.coords,
                ) {
                    edges.push(edge);
                }
            }
        }

        let edges_with_waypoints = edges.iter().filter(|e| !e.waypoints.is_empty()).count();
        let total_waypoints: usize = edges.iter().map(|e| e.waypoints.len()).sum();

        tracing::info!(
            "Built {} edges from {} ways: {} edges have waypoints ({:.1}%), {} total waypoints",
            edges.len(),
            ways_count,
            edges_with_waypoints,
            (edges_with_waypoints as f64 / edges.len() as f64) * 100.0,
            total_waypoints
        );

        Ok(GraphFile {
            nodes: node_state.nodes,
            edges,
        })
    }

    fn collect_nodes(&self, path: &Path) -> Result<NodeCollectionState, GraphBuildError> {
        let bbox = self.config.bbox;

        // If no bbox, collect all nodes
        if bbox.is_none() {
            let reader = ElementReader::from_path(path)?;
            let osm_nodes = reader.par_map_reduce(
                |element| -> Vec<OsmNode> {
                    match element {
                        Element::Node(node) => {
                            let elevation = extract_elevation(&node.tags().collect::<Vec<_>>());
                            vec![OsmNode {
                                osm_id: node.id(),
                                lat: node.lat(),
                                lon: node.lon(),
                                elevation,
                            }]
                        }
                        Element::DenseNode(node) => {
                            let elevation = extract_elevation(&node.tags().collect::<Vec<_>>());
                            vec![OsmNode {
                                osm_id: node.id(),
                                lat: node.lat(),
                                lon: node.lon(),
                                elevation,
                            }]
                        }
                        _ => Vec::new(),
                    }
                },
                Vec::new,
                |mut acc, nodes| {
                    acc.extend(nodes);
                    acc
                },
            )?;

            let state = osm_nodes
                .into_iter()
                .fold(NodeCollectionState::new(), |state, node| {
                    state.with_node(node.osm_id, node.lat, node.lon, node.elevation)
                });
            return Ok(state);
        }

        // WITH BBOX: 3-pass approach to maintain connectivity
        let bbox = bbox.unwrap();

        // PASS 1: Collect nodes IN bbox to identify which ways touch the bbox
        let reader_nodes_in_bbox = ElementReader::from_path(path)?;
        let nodes_in_bbox: std::collections::HashSet<i64> = reader_nodes_in_bbox
            .par_map_reduce(
                |element| -> Vec<i64> {
                    match element {
                        Element::Node(node) => {
                            let coord = Coordinate {
                                lat: node.lat(),
                                lon: node.lon(),
                            };
                            if bbox.contains(coord) {
                                vec![node.id()]
                            } else {
                                Vec::new()
                            }
                        }
                        Element::DenseNode(node) => {
                            let coord = Coordinate {
                                lat: node.lat(),
                                lon: node.lon(),
                            };
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
                |mut acc, nodes| {
                    acc.extend(nodes);
                    acc
                },
            )?
            .into_iter()
            .collect();

        // PASS 2: Find ways that have at least one node in bbox
        let reader_ways = ElementReader::from_path(path)?;
        let needed_node_ids: std::collections::HashSet<i64> = reader_ways
            .par_map_reduce(
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
                |mut acc, nodes| {
                    acc.extend(nodes);
                    acc
                },
            )?
            .into_iter()
            .collect();

        // PASS 3: Collect actual node data for the needed nodes
        let reader = ElementReader::from_path(path)?;
        let osm_nodes = reader.par_map_reduce(
            |element| -> Vec<OsmNode> {
                match element {
                    Element::Node(node) => {
                        if needed_node_ids.contains(&node.id()) {
                            let elevation = extract_elevation(&node.tags().collect::<Vec<_>>());
                            vec![OsmNode {
                                osm_id: node.id(),
                                lat: node.lat(),
                                lon: node.lon(),
                                elevation,
                            }]
                        } else {
                            Vec::new()
                        }
                    }
                    Element::DenseNode(node) => {
                        if needed_node_ids.contains(&node.id()) {
                            let elevation = extract_elevation(&node.tags().collect::<Vec<_>>());
                            vec![OsmNode {
                                osm_id: node.id(),
                                lat: node.lat(),
                                lon: node.lon(),
                                elevation,
                            }]
                        } else {
                            Vec::new()
                        }
                    }
                    _ => Vec::new(),
                }
            },
            Vec::new,
            |mut acc, nodes| {
                acc.extend(nodes);
                acc
            },
        )?;

        let state = osm_nodes
            .into_iter()
            .fold(NodeCollectionState::new(), |state, node| {
                state.with_node(node.osm_id, node.lat, node.lon, node.elevation)
            });

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

// Pure function to process a node element (not used anymore, kept for reference)
#[allow(dead_code)]
fn process_node_element(
    lat: f64,
    lon: f64,
    osm_id: i64,
    elevation: Option<f64>,
    bbox: Option<BoundingBox>,
) -> Vec<OsmNode> {
    let coord = Coordinate { lat, lon };

    // Apply bounding box filter
    let in_bbox = bbox.map(|b| b.contains(coord)).unwrap_or(true);

    if in_bbox {
        vec![OsmNode {
            osm_id,
            lat,
            lon,
            elevation,
        }]
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
        .filter_map(|pair| create_edge_record(pair[0], pair[1], surface, coords, osm_to_graph))
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
        waypoints: Vec::new(), // No intermediate waypoints for now
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
    let tags_map: HashMap<&str, &str> =
        tags.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

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
            "service" | "residential" | "primary" | "secondary" | "tertiary" => SurfaceType::Paved,
            _ => SurfaceType::Trail,
        };
    }

    // Default fallback
    SurfaceType::Trail
}

// Extract elevation from OSM tags
// OSM uses 'ele' tag for elevation in meters
fn extract_elevation(tags: &[(&str, &str)]) -> Option<f64> {
    tags.iter()
        .find(|(k, _)| *k == "ele")
        .and_then(|(_, v)| v.parse::<f64>().ok())
}

/// Identify intersection nodes in OSM ways
/// A node is an intersection if:
/// - It appears in more than one way (crossroad)
/// - OR it's an endpoint (start/end) of a way
///
/// This is used to determine where to create graph edges with intermediate waypoints.
fn identify_intersection_nodes(ways: &[OsmWay]) -> std::collections::HashSet<i64> {
    use std::collections::{HashMap, HashSet};

    // Count how many times each node appears
    let mut node_count: HashMap<i64, usize> = HashMap::new();
    let mut endpoints: HashSet<i64> = HashSet::new();

    for (_, node_refs, _) in ways {
        if node_refs.is_empty() {
            continue;
        }

        // Mark endpoints
        if let Some(&first) = node_refs.first() {
            endpoints.insert(first);
        }
        if let Some(&last) = node_refs.last() {
            endpoints.insert(last);
        }

        // Count all nodes
        for &node_id in node_refs {
            *node_count.entry(node_id).or_insert(0) += 1;
        }
    }

    // A node is an intersection if it appears in multiple ways OR is an endpoint
    let mut intersections = HashSet::new();
    for (&node_id, &count) in &node_count {
        if count > 1 || endpoints.contains(&node_id) {
            intersections.insert(node_id);
        }
    }

    intersections
}

/// Build an edge with intermediate waypoints from a segment of OSM nodes
/// Returns None if the segment is invalid (< 2 nodes, nodes not found, etc.)
fn build_edge_with_waypoints(
    node_refs: &[i64],
    surface: SurfaceType,
    osm_to_graph: &HashMap<i64, u64>,
    coords: &[Coordinate],
) -> Option<EdgeRecord> {
    if node_refs.len() < 2 {
        return None;
    }

    let first_osm = node_refs[0];
    let last_osm = node_refs[node_refs.len() - 1];

    // Get graph IDs for start and end
    let from_id = *osm_to_graph.get(&first_osm)?;
    let to_id = *osm_to_graph.get(&last_osm)?;

    // Bounds check
    if (from_id as usize) >= coords.len() || (to_id as usize) >= coords.len() {
        return None;
    }

    let from_coord = coords[from_id as usize];
    let to_coord = coords[to_id as usize];

    // Collect intermediate waypoints (excluding first and last)
    let waypoints: Vec<Coordinate> = node_refs[1..node_refs.len() - 1]
        .iter()
        .filter_map(|&osm_id| {
            let graph_id = *osm_to_graph.get(&osm_id)?;
            if (graph_id as usize) < coords.len() {
                Some(coords[graph_id as usize])
            } else {
                None
            }
        })
        .collect();

    // Calculate total length along the waypoints
    let mut length_m = 0.0;
    let mut prev_coord = from_coord;

    for &waypoint in &waypoints {
        length_m += crate::routing::haversine_km(prev_coord, waypoint) * 1000.0;
        prev_coord = waypoint;
    }
    length_m += crate::routing::haversine_km(prev_coord, to_coord) * 1000.0;

    Some(EdgeRecord {
        from: from_id,
        to: to_id,
        surface,
        length_m,
        waypoints,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identify_intersections_simple_way() {
        // Simple way with 5 nodes: only endpoints should be intersections
        let ways = vec![
            (1, vec![10, 20, 30, 40, 50], vec![]),
        ];

        let intersections = identify_intersection_nodes(&ways);

        // Should have 2 intersections: start (10) and end (50)
        assert_eq!(intersections.len(), 2);
        assert!(intersections.contains(&10));
        assert!(intersections.contains(&50));
        assert!(!intersections.contains(&20));
        assert!(!intersections.contains(&30));
        assert!(!intersections.contains(&40));
    }

    #[test]
    fn test_identify_intersections_t_junction() {
        // T-junction: way1 crosses way2 at node 30
        let ways = vec![
            (1, vec![10, 20, 30, 40, 50], vec![]),
            (2, vec![60, 70, 30, 80], vec![]),
        ];

        let intersections = identify_intersection_nodes(&ways);

        // Should have 7 intersections:
        // - All endpoints: 10, 50, 60, 80
        // - Node 30 (appears in both ways)
        assert!(intersections.contains(&10)); // endpoint way1
        assert!(intersections.contains(&50)); // endpoint way1
        assert!(intersections.contains(&60)); // endpoint way2
        assert!(intersections.contains(&80)); // endpoint way2
        assert!(intersections.contains(&30)); // crossroad

        // Non-intersection nodes
        assert!(!intersections.contains(&20));
        assert!(!intersections.contains(&40));
        assert!(!intersections.contains(&70));
    }

    #[test]
    fn test_identify_intersections_empty_ways() {
        let ways: Vec<OsmWay> = vec![];
        let intersections = identify_intersection_nodes(&ways);
        assert_eq!(intersections.len(), 0);
    }

    #[test]
    fn test_identify_intersections_single_node_way() {
        // Way with only one node (edge case)
        let ways = vec![
            (1, vec![10], vec![]),
        ];

        let intersections = identify_intersection_nodes(&ways);

        // Single node is both start and end, so it's an intersection
        assert_eq!(intersections.len(), 1);
        assert!(intersections.contains(&10));
    }

    #[test]
    fn test_build_edge_with_waypoints_two_nodes() {
        // Segment with 2 nodes (no intermediate waypoints)
        let node_refs = vec![10, 20];

        let mut osm_to_graph = HashMap::new();
        osm_to_graph.insert(10, 0);
        osm_to_graph.insert(20, 1);

        let coords = vec![
            Coordinate { lat: 45.0, lon: 4.0 },
            Coordinate { lat: 45.01, lon: 4.01 },
        ];

        let edge = build_edge_with_waypoints(
            &node_refs,
            SurfaceType::Paved,
            &osm_to_graph,
            &coords,
        ).expect("Should create edge");

        assert_eq!(edge.from, 0);
        assert_eq!(edge.to, 1);
        assert_eq!(edge.waypoints.len(), 0); // No intermediate waypoints
        assert!(edge.length_m > 0.0);
    }

    #[test]
    fn test_build_edge_with_waypoints_five_nodes() {
        // Segment with 5 nodes (3 intermediate waypoints)
        let node_refs = vec![10, 20, 30, 40, 50];

        let mut osm_to_graph = HashMap::new();
        osm_to_graph.insert(10, 0);
        osm_to_graph.insert(20, 1);
        osm_to_graph.insert(30, 2);
        osm_to_graph.insert(40, 3);
        osm_to_graph.insert(50, 4);

        let coords = vec![
            Coordinate { lat: 45.0, lon: 4.0 },
            Coordinate { lat: 45.01, lon: 4.01 },
            Coordinate { lat: 45.02, lon: 4.02 },
            Coordinate { lat: 45.03, lon: 4.03 },
            Coordinate { lat: 45.04, lon: 4.04 },
        ];

        let edge = build_edge_with_waypoints(
            &node_refs,
            SurfaceType::Trail,
            &osm_to_graph,
            &coords,
        ).expect("Should create edge");

        assert_eq!(edge.from, 0);
        assert_eq!(edge.to, 4);
        assert_eq!(edge.waypoints.len(), 3); // 3 intermediate waypoints

        // Check waypoints are correct
        assert_eq!(edge.waypoints[0].lat, 45.01);
        assert_eq!(edge.waypoints[1].lat, 45.02);
        assert_eq!(edge.waypoints[2].lat, 45.03);

        // Length should be sum of segments
        assert!(edge.length_m > 0.0);
    }

    #[test]
    fn test_build_edge_with_waypoints_invalid() {
        // Test with less than 2 nodes
        let node_refs = vec![10];
        let osm_to_graph = HashMap::new();
        let coords = vec![Coordinate { lat: 45.0, lon: 4.0 }];

        let edge = build_edge_with_waypoints(
            &node_refs,
            SurfaceType::Paved,
            &osm_to_graph,
            &coords,
        );

        assert!(edge.is_none());
    }

    #[test]
    fn test_build_edge_with_waypoints_missing_node() {
        // Test with node not in osm_to_graph mapping
        let node_refs = vec![10, 20];

        let mut osm_to_graph = HashMap::new();
        osm_to_graph.insert(10, 0);
        // 20 is missing

        let coords = vec![
            Coordinate { lat: 45.0, lon: 4.0 },
            Coordinate { lat: 45.01, lon: 4.01 },
        ];

        let edge = build_edge_with_waypoints(
            &node_refs,
            SurfaceType::Paved,
            &osm_to_graph,
            &coords,
        );

        assert!(edge.is_none());
    }
}
