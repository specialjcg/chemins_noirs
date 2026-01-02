use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{graph::GraphBuilder, models::Coordinate};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialGraphRequest {
    pub start: Coordinate,
    pub end: Coordinate,
    #[serde(default = "default_margin")]
    pub margin_km: f64,
}

fn default_margin() -> f64 {
    5.0 // 5km default margin
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialGraphResponse {
    pub nodes: Vec<crate::graph::NodeRecord>,
    pub edges: Vec<crate::graph::EdgeRecord>,
    pub bbox: BBoxInfo,
    pub cache_key: String,
    pub stats: GraphStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BBoxInfo {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub from_cache: bool,
}

pub struct PartialGraphConfig {
    pub pbf_path: std::path::PathBuf,
    pub cache_dir: std::path::PathBuf,
    pub tiles_dir: Option<std::path::PathBuf>,
}

/// Handler for POST /api/graph/partial
///
/// Generates a small partial graph (KB-MB) instead of loading
/// the full 600MB graph, based on user's selected route points.
pub async fn partial_graph_handler(
    State(config): State<Arc<PartialGraphConfig>>,
    Json(req): Json<PartialGraphRequest>,
) -> Result<Json<PartialGraphResponse>, (StatusCode, String)> {
    tracing::info!(
        "Partial graph request: ({}, {}) -> ({}, {}) with {}km margin",
        req.start.lat,
        req.start.lon,
        req.end.lat,
        req.end.lon,
        req.margin_km
    );

    // Validate coordinates
    if !is_valid_coordinate(&req.start) || !is_valid_coordinate(&req.end) {
        return Err((StatusCode::BAD_REQUEST, "Invalid coordinates".to_string()));
    }

    // Check if cache directory exists
    let cache_path = std::path::Path::new(&config.cache_dir);
    let from_cache = cache_path
        .join(format!(
            "partial_{}.json",
            crate::graph::BoundingBox::from_route(req.start, req.end, req.margin_km).cache_key()
        ))
        .exists();

    // Build partial graph (with caching)
    let graph = GraphBuilder::build_partial_cached(
        &config.pbf_path,
        &config.cache_dir,
        req.start,
        req.end,
        req.margin_km,
    )
    .map_err(|e| {
        tracing::error!("Failed to build partial graph: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Graph generation failed: {}", e),
        )
    })?;

    let bbox = crate::graph::BoundingBox::from_route(req.start, req.end, req.margin_km);

    Ok(Json(PartialGraphResponse {
        cache_key: bbox.cache_key(),
        stats: GraphStats {
            node_count: graph.nodes.len(),
            edge_count: graph.edges.len(),
            from_cache,
        },
        bbox: BBoxInfo {
            min_lat: bbox.min_lat,
            max_lat: bbox.max_lat,
            min_lon: bbox.min_lon,
            max_lon: bbox.max_lon,
        },
        nodes: graph.nodes,
        edges: graph.edges,
    }))
}

fn is_valid_coordinate(coord: &Coordinate) -> bool {
    coord.lat >= -90.0 && coord.lat <= 90.0 && coord.lon >= -180.0 && coord.lon <= 180.0
}
