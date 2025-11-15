pub mod engine;
pub mod error;
pub mod gpx_export;
pub mod graph;
pub mod models;
pub mod partial_graph;
pub mod routing;

use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};

use crate::engine::RouteEngine;
use crate::error::RouteError;
use crate::gpx_export::encode_route_as_gpx;
use crate::models::{
    ApiError, Coordinate, RouteBounds, RouteMetadata, RouteRequest, RouteResponse,
};
use crate::routing::{approximate_distance_km, generate_route};

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<RouteEngine>,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/route", post(route_handler))
        .with_state(state)
}

/// Create router with partial graph support
pub fn create_router_with_partial(
    state: AppState,
    partial_config: Arc<partial_graph::PartialGraphConfig>,
) -> Router {
    Router::new()
        .route("/api/route", post(route_handler))
        .with_state(state.clone())
        .route("/api/graph/partial", post(partial_graph::partial_graph_handler))
        .with_state(partial_config)
}

async fn route_handler(
    State(state): State<AppState>,
    Json(req): Json<RouteRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let path = state
        .engine
        .find_path(&req)
        .unwrap_or_else(|| generate_route(&req));
    let distance_km = approximate_distance_km(&path);
    let gpx_base64 = encode_route_as_gpx(&path).map_err(internal_error)?;
    let metadata = build_metadata(&path);

    let response = RouteResponse {
        path,
        distance_km,
        gpx_base64,
        metadata: Some(metadata),
    };

    Ok(Json(response))
}

fn build_metadata(path: &[Coordinate]) -> RouteMetadata {
    let mut min_lat = f64::MAX;
    let mut max_lat = f64::MIN;
    let mut min_lon = f64::MAX;
    let mut max_lon = f64::MIN;

    for coord in path {
        min_lat = min_lat.min(coord.lat);
        max_lat = max_lat.max(coord.lat);
        min_lon = min_lon.min(coord.lon);
        max_lon = max_lon.max(coord.lon);
    }

    RouteMetadata {
        point_count: path.len(),
        bounds: RouteBounds {
            min_lat,
            max_lat,
            min_lon,
            max_lon,
        },
        start: path
            .first()
            .copied()
            .unwrap_or(Coordinate { lat: 0.0, lon: 0.0 }),
        end: path
            .last()
            .copied()
            .unwrap_or(Coordinate { lat: 0.0, lon: 0.0 }),
    }
}

fn internal_error(err: RouteError) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            message: err.to_string(),
        }),
    )
}
