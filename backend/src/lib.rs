pub mod engine;
pub mod error;
pub mod gpx_export;
pub mod graph;
pub mod models;
pub mod routing;

use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};

use crate::engine::RouteEngine;
use crate::error::RouteError;
use crate::gpx_export::encode_route_as_gpx;
use crate::models::{RouteRequest, RouteResponse};
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

    let response = RouteResponse {
        path,
        distance_km,
        gpx_base64,
    };

    Ok(Json(response))
}

#[derive(serde::Serialize)]
struct ApiError {
    message: String,
}

fn internal_error(err: RouteError) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            message: err.to_string(),
        }),
    )
}
