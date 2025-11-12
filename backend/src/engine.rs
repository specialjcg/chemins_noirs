use std::{
    collections::HashMap,
    fs,
    io::{self, Read},
    path::Path,
};

use petgraph::{
    algo::astar,
    graph::{NodeIndex, UnGraph},
};
use serde::Deserialize;

use crate::models::{Coordinate, RouteRequest, SurfaceType};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("failed to read graph file: {0}")]
    Io(#[from] io::Error),
    #[error("invalid graph definition: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("graph is empty")]
    EmptyGraph,
    #[error("edge references unknown node {0}")]
    MissingNode(u32),
}

#[derive(Clone)]
pub struct RouteEngine {
    graph: UnGraph<NodeData, EdgeData>,
    nodes: Vec<NodeData>,
}

#[derive(Clone, Debug, Deserialize)]
struct GraphFile {
    nodes: Vec<NodeRecord>,
    edges: Vec<EdgeRecord>,
}

#[derive(Clone, Debug, Deserialize)]
struct NodeRecord {
    id: u32,
    lat: f64,
    lon: f64,
    #[serde(default)]
    population_density: f64,
}

#[derive(Clone, Debug, Deserialize)]
struct EdgeRecord {
    from: u32,
    to: u32,
    surface: SurfaceType,
    length_m: f64,
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
        let data = fs::read_to_string(path)?;
        Self::from_reader(data.as_bytes())
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
        let start = self.closest_node(req.start)?;
        let end = self.closest_node(req.end)?;
        let weights = WeightConfig {
            population: req.w_pop,
            paved: req.w_paved,
        };

        let heuristic = |idx: NodeIndex| straight_line_km(self.nodes[idx.index()].coord, req.end);
        let edge_cost =
            |edge: petgraph::graph::EdgeReference<EdgeData>| self.edge_cost(edge.weight(), weights);

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

    fn closest_node(&self, target: Coordinate) -> Option<NodeIndex> {
        self.nodes
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = squared_distance(a.coord, target);
                let db = squared_distance(b.coord, target);
                da.partial_cmp(&db).unwrap()
            })
            .map(|(idx, _)| NodeIndex::new(idx))
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
}
