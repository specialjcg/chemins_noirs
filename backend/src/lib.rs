pub mod dem;
pub mod elevation;
pub mod engine;
pub mod error;
pub mod gpx_export;
pub mod graph;
pub mod loops;
pub mod models;
pub mod partial_graph;
pub mod routing;
pub mod terrain;

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use tower_http::cors::{Any, CorsLayer};

use crate::engine::RouteEngine;
use crate::error::RouteError;
use crate::gpx_export::encode_route_as_gpx;
use crate::loops::LoopGenerationError;
use crate::models::{
    ApiError, Coordinate, LoopRouteRequest, RouteBounds, RouteMetadata, RouteRequest, RouteResponse,
};
use crate::routing::{approximate_distance_km, generate_route};

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<RouteEngine>,
}

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/route", post(route_handler))
        .route("/api/loops", post(loop_route_handler))
        .route("/api/routes/save", post(save_route_handler))
        .route("/api/routes/load", get(load_route_handler))
        .layer(cors)
        .with_state(state)
}

/// Create router with partial graph support
pub fn create_router_with_partial(
    state: AppState,
    partial_config: Arc<partial_graph::PartialGraphConfig>,
) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/route", post(route_handler))
        .route("/api/loops", post(loop_route_handler))
        .route("/api/routes/save", post(save_route_handler))
        .route("/api/routes/load", get(load_route_handler))
        .with_state(state.clone())
        .route(
            "/api/graph/partial",
            post(partial_graph::partial_graph_handler),
        )
        .with_state(partial_config)
        .layer(cors)
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
        elevation_profile: None, // Not available in this handler (legacy backend)
        terrain: None,
    };

    Ok(Json(response))
}

async fn loop_route_handler(
    State(state): State<AppState>,
    Json(req): Json<LoopRouteRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    loops::generate_loops(&state.engine, &req)
        .await
        .map(Json)
        .map_err(loop_error)
}

pub(crate) fn build_metadata(path: &[Coordinate]) -> RouteMetadata {
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

fn loop_error(err: LoopGenerationError) -> (StatusCode, Json<ApiError>) {
    let status = match err {
        LoopGenerationError::InvalidTargetDistance => StatusCode::BAD_REQUEST,
        LoopGenerationError::NoLoopFound => StatusCode::NOT_FOUND,
        LoopGenerationError::Gpx(_) | LoopGenerationError::Elevation(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };

    (
        status,
        Json(ApiError {
            message: err.to_string(),
        }),
    )
}

// Handler pour sauvegarder une route sur le disque
async fn save_route_handler(
    State(_state): State<AppState>,
    Json(route): Json<RouteResponse>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let save_dir = PathBuf::from("backend/data/saved_routes");

    // Créer le répertoire s'il n'existe pas
    if let Err(e) = std::fs::create_dir_all(&save_dir) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to create save directory: {}", e),
            }),
        ));
    }

    // Utiliser un nom de fichier fixe pour la dernière route sauvegardée
    let file_path = save_dir.join("last_route.json");

    // Sérialiser et sauvegarder
    let json_str = serde_json::to_string_pretty(&route).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to serialize route: {}", e),
            }),
        )
    })?;

    std::fs::write(&file_path, json_str).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to write route file: {}", e),
            }),
        )
    })?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Route sauvegardée avec succès"
    })))
}

// Handler pour charger la dernière route sauvegardée
async fn load_route_handler(
    State(_state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let file_path = PathBuf::from("backend/data/saved_routes/last_route.json");

    // Vérifier si le fichier existe
    if !file_path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                message: "Aucune route sauvegardée trouvée".to_string(),
            }),
        ));
    }

    // Lire et désérialiser
    let json_str = std::fs::read_to_string(&file_path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to read route file: {}", e),
            }),
        )
    })?;

    let route: RouteResponse = serde_json::from_str(&json_str).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to deserialize route: {}", e),
            }),
        )
    })?;

    Ok(Json(route))
}
