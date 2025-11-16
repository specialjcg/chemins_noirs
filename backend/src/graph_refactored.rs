use std::{
    collections::HashMap,
    fs::File,
    io::{self, Write},
    path::Path,
};

use osmpbf::{Element, ElementReader};
use serde::{Deserialize, Serialize};

use crate::models::{Coordinate, SurfaceType};

// ============================================================================
// Data Structures (Immutable by design)
// ============================================================================

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

#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

// ============================================================================
// Pure Functions - BoundingBox
// ============================================================================

impl BoundingBox {
    pub fn contains(&self, coord: Coordinate) -> bool {
        coord.lat >= self.min_lat
            && coord.lat <= self.max_lat
            && coord.lon >= self.min_lon
            && coord.lon <= self.max_lon
    }
}

// ============================================================================
// GraphFile - I/O Operations (Side effects isolated)
// ============================================================================

impl GraphFile {
    pub fn read_from_path(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let file = File::open(path)?;
        serde_json::from_reader(file).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    pub fn write_to_path(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        let mut file = File::create(path)?;
        serde_json::to_writer_pretty(&mut file, self)?;
        file.flush()
    }
}

// ============================================================================
// Error Handling
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum GraphBuildError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("osm pbf error: {0}")]
    OsmPbf(#[from] osmpbf::Error),
    #[error("invalid graph definition: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("graph contains no nodes")]
    EmptyGraph,
}

// ============================================================================
// Strategy Pattern - Surface Type Inference
// ============================================================================

/// Strategy trait for inferring surface types from OSM tags
pub trait SurfaceInferenceStrategy {
    fn infer_surface(&self, tags: &[(String, String)]) -> SurfaceType;
}

/// Default surface inference strategy based on highway and surface tags
pub struct DefaultSurfaceStrategy;

impl SurfaceInferenceStrategy for DefaultSurfaceStrategy {
    fn infer_surface(&self, tags: &[(String, String)]) -> SurfaceType {
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
}

// ============================================================================
// Pure Functions - Highway Filtering
// ============================================================================

/// Pure function to check if a highway tag is supported
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

/// Pure function to check if way has supported highway tag
fn has_supported_highway(tags: &[(String, String)]) -> bool {
    tags.iter()
        .find(|(k, _)| k == "highway")
        .map(|(_, v)| is_supported_highway(v))
        .unwrap_or(false)
}

// ============================================================================
// Builder Pattern - Graph Builder Configuration
// ============================================================================

pub struct GraphBuilderConfig {
    pub bbox: Option<BoundingBox>,
    pub surface_strategy: Box<dyn SurfaceInferenceStrategy>,
}

impl Default for GraphBuilderConfig {
    fn default() -> Self {
        Self {
            bbox: None,
            surface_strategy: Box::new(DefaultSurfaceStrategy),
        }
    }
}

impl GraphBuilderConfig {
    /// Builder method for setting bounding box
    pub fn with_bbox(mut self, bbox: BoundingBox) -> Self {
        self.bbox = Some(bbox);
        self
    }

    /// Builder method for custom surface inference strategy
    pub fn with_surface_strategy(
        mut self,
        strategy: Box<dyn SurfaceInferenceStrategy>,
    ) -> Self {
        self.surface_strategy = strategy;
        self
    }
}

// ============================================================================
// Internal Data Structures for Processing
// ============================================================================

#[derive(Debug, Clone)]
struct OsmNode {
    osm_id: i64,
    graph_id: u64,
    lat: f64,
    lon: f64,
}

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

    /// Pure function to add a node to the collection state
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

// ============================================================================
// Graph Builder - Main Logic
// ============================================================================

pub struct GraphBuilder {
    config: GraphBuilderConfig,
}

impl GraphBuilder {
    pub fn new(config: GraphBuilderConfig) -> Self {
        Self { config }
    }

    /// Build graph from PBF file (main entry point)
    pub fn build_from_pbf(&self, path: impl AsRef<Path>) -> Result<GraphFile, GraphBuildError> {
        let path = path.as_ref();

        // Single-pass collection with functional approach
        let reader = ElementReader::from_path(path)?;

        // Collect nodes first
        let node_state = self.collect_nodes(&reader)?;

        if node_state.nodes.is_empty() {
            return Err(GraphBuildError::EmptyGraph);
        }

        // Collect edges using collected nodes
        let reader = ElementReader::from_path(path)?;
        let edges = self.collect_edges(&reader, &node_state)?;

        Ok(GraphFile {
            nodes: node_state.nodes,
            edges,
        })
    }

    /// Pure functional node collection
    fn collect_nodes(
        &self,
        reader: &ElementReader,
    ) -> Result<NodeCollectionState, GraphBuildError> {
        let bbox = self.config.bbox;

        // Use fold to accumulate state immutably
        let result = reader.par_map_reduce(
            |element| -> Vec<OsmNode> {
                if let Element::Node(node) = element {
                    let coord = Coordinate {
                        lat: node.lat(),
                        lon: node.lon(),
                    };

                    // Apply bounding box filter
                    let in_bbox = bbox.map(|b| b.contains(coord)).unwrap_or(true);

                    if in_bbox {
                        return vec![OsmNode {
                            osm_id: node.id(),
                            graph_id: 0, // Will be assigned during reduction
                            lat: node.lat(),
                            lon: node.lon(),
                        }];
                    }
                }
                Vec::new()
            },
            Vec::new,
            |mut acc, nodes| {
                acc.extend(nodes);
                acc
            },
        )?;

        // Convert collected nodes to state
        let state = result.into_iter().fold(
            NodeCollectionState::new(),
            |state, node| state.with_node(node.osm_id, node.lat, node.lon),
        );

        Ok(state)
    }

    /// Pure functional edge collection
    fn collect_edges(
        &self,
        reader: &ElementReader,
        node_state: &NodeCollectionState,
    ) -> Result<Vec<EdgeRecord>, GraphBuildError> {
        let surface_strategy = &self.config.surface_strategy;
        let osm_to_graph = &node_state.osm_to_graph_id;
        let coords = &node_state.coords;

        let edges = reader.par_map_reduce(
            |element| -> Vec<EdgeRecord> {
                if let Element::Way(way) = element {
                    // Collect tags into vec
                    let tags: Vec<(String, String)> = way.tags().collect();

                    // Filter by highway type
                    if !has_supported_highway(&tags) {
                        return Vec::new();
                    }

                    // Infer surface type
                    let surface = surface_strategy.infer_surface(&tags);

                    // Collect node references
                    let node_refs: Vec<i64> = way.refs().collect();

                    // Create edges for consecutive node pairs
                    return node_refs
                        .windows(2)
                        .filter_map(|pair| {
                            let from_osm = pair[0];
                            let to_osm = pair[1];

                            // Look up graph IDs
                            let from = osm_to_graph.get(&from_osm)?;
                            let to = osm_to_graph.get(&to_osm)?;

                            // Calculate distance
                            let coord_a = coords.get((from - 1) as usize)?;
                            let coord_b = coords.get((to - 1) as usize)?;
                            let length_km = crate::routing::haversine_km(*coord_a, *coord_b);

                            Some(EdgeRecord {
                                from: *from,
                                to: *to,
                                surface,
                                length_m: length_km * 1000.0,
                            })
                        })
                        .collect();
                }
                Vec::new()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bbox_contains() {
        let bbox = BoundingBox {
            min_lat: 45.0,
            max_lat: 46.0,
            min_lon: 5.0,
            max_lon: 6.0,
        };

        assert!(bbox.contains(Coordinate { lat: 45.5, lon: 5.5 }));
        assert!(!bbox.contains(Coordinate { lat: 44.5, lon: 5.5 }));
        assert!(!bbox.contains(Coordinate { lat: 45.5, lon: 4.5 }));
    }

    #[test]
    fn test_is_supported_highway() {
        assert!(is_supported_highway("path"));
        assert!(is_supported_highway("residential"));
        assert!(!is_supported_highway("motorway"));
    }

    #[test]
    fn test_default_surface_strategy() {
        let strategy = DefaultSurfaceStrategy;

        // Test explicit surface tags
        let tags = vec![("surface".to_string(), "gravel".to_string())];
        assert!(matches!(
            strategy.infer_surface(&tags),
            SurfaceType::Trail
        ));

        let tags = vec![("surface".to_string(), "asphalt".to_string())];
        assert!(matches!(
            strategy.infer_surface(&tags),
            SurfaceType::Paved
        ));

        // Test highway fallback
        let tags = vec![("highway".to_string(), "path".to_string())];
        assert!(matches!(
            strategy.infer_surface(&tags),
            SurfaceType::Trail
        ));

        let tags = vec![("highway".to_string(), "residential".to_string())];
        assert!(matches!(
            strategy.infer_surface(&tags),
            SurfaceType::Paved
        ));
    }

    #[test]
    fn test_node_collection_state() {
        let state = NodeCollectionState::new()
            .with_node(100, 45.5, 5.5)
            .with_node(200, 46.0, 6.0);

        assert_eq!(state.nodes.len(), 2);
        assert_eq!(state.coords.len(), 2);
        assert_eq!(*state.osm_to_graph_id.get(&100).unwrap(), 1);
        assert_eq!(*state.osm_to_graph_id.get(&200).unwrap(), 2);
    }
}
