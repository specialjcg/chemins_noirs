use std::{
    collections::HashMap,
    fs::File,
    io::{self, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::models::{Coordinate, SurfaceType};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphFile {
    pub nodes: Vec<NodeRecord>,
    pub edges: Vec<EdgeRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeRecord {
    pub id: u32,
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub population_density: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeRecord {
    pub from: u32,
    pub to: u32,
    pub surface: SurfaceType,
    pub length_m: f64,
}

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
}

#[derive(Debug, thiserror::Error)]
pub enum GraphBuildError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("pbf error: {0}")]
    Osm(#[from] osmpbf::Error),
    #[error("invalid graph definition: {0}")]
    Parse(#[from] serde_json::Error),
}

pub struct GraphBuilderConfig {
    pub bbox: Option<BoundingBox>,
}

impl Default for GraphBuilderConfig {
    fn default() -> Self {
        Self { bbox: None }
    }
}

pub struct GraphBuilder {
    config: GraphBuilderConfig,
}

impl GraphBuilder {
    pub fn new(config: GraphBuilderConfig) -> Self {
        Self { config }
    }

    pub fn build_from_pbf(&self, path: impl AsRef<Path>) -> Result<GraphFile, GraphBuildError> {
        let path = path.as_ref();
        let node_positions = self.collect_nodes(path)?;
        self.build_edges(path, node_positions)
    }

    fn collect_nodes(&self, path: &Path) -> Result<HashMap<i64, Coordinate>, GraphBuildError> {
        use osmpbf::{Element, ElementReader};

        let mut map = HashMap::new();
        let reader = ElementReader::from_path(path)?;
        reader.for_each(|element| {
            if let Element::Node(node) = element {
                let coord = Coordinate {
                    lat: node.lat(),
                    lon: node.lon(),
                };
                if self
                    .config
                    .bbox
                    .map(|bbox| bbox.contains(coord))
                    .unwrap_or(true)
                {
                    map.insert(node.id(), coord);
                }
            }
        })?;
        Ok(map)
    }

    fn build_edges(
        &self,
        path: &Path,
        node_positions: HashMap<i64, Coordinate>,
    ) -> Result<GraphFile, GraphBuildError> {
        use osmpbf::{Element, ElementReader};

        let mut nodes: Vec<NodeRecord> = Vec::new();
        let mut edges: Vec<EdgeRecord> = Vec::new();
        let mut osm_to_graph: HashMap<i64, u32> = HashMap::new();
        let mut coords_by_id: Vec<Coordinate> = Vec::new();

        let reader = ElementReader::from_path(path)?;
        reader.for_each(|element| {
            if let Element::Way(way) = element {
                if !is_supported_highway(&way) {
                    return;
                }
                let highway = way.tags().find(|(k, _)| *k == "highway").map(|(_, v)| v);
                let surface = infer_surface(&way, highway);
                let mut last_id: Option<u32> = None;
                for node_ref in way.refs() {
                    let graph_id = match osm_to_graph.get(&node_ref) {
                        Some(id) => *id,
                        None => match node_positions.get(&node_ref) {
                            Some(coord) => {
                                let new_id = nodes.len() as u32 + 1;
                                osm_to_graph.insert(node_ref, new_id);
                                nodes.push(NodeRecord {
                                    id: new_id,
                                    lat: coord.lat,
                                    lon: coord.lon,
                                    population_density: 0.0,
                                });
                                coords_by_id.push(*coord);
                                new_id
                            }
                            None => continue,
                        },
                    };

                    if let Some(prev) = last_id {
                        let coord_a = coords_by_id[(prev - 1) as usize];
                        let coord_b = coords_by_id[(graph_id - 1) as usize];
                        let length_km = crate::routing::haversine_km(coord_a, coord_b);
                        edges.push(EdgeRecord {
                            from: prev,
                            to: graph_id,
                            surface,
                            length_m: length_km * 1000.0,
                        });
                    }

                    last_id = Some(graph_id);
                }
            }
        })?;

        Ok(GraphFile { nodes, edges })
    }
}

fn is_supported_highway(way: &osmpbf::Way) -> bool {
    let mut highway = None;
    for (k, v) in way.tags() {
        if k == "highway" {
            highway = Some(v);
            break;
        }
    }
    matches!(
        highway,
        Some(
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
    )
}

fn infer_surface(way: &osmpbf::Way, highway: Option<&str>) -> SurfaceType {
    for (k, v) in way.tags() {
        if k == "surface" {
            return match v {
                "gravel" | "fine_gravel" | "compacted" | "unpaved" => SurfaceType::Trail,
                "dirt" | "earth" | "ground" | "grass" => SurfaceType::Dirt,
                _ => SurfaceType::Paved,
            };
        }
    }
    match highway {
        Some("path" | "footway" | "track") => SurfaceType::Trail,
        Some("service" | "residential" | "primary" | "secondary" | "tertiary") => {
            SurfaceType::Paved
        }
        _ => SurfaceType::Trail,
    }
}
