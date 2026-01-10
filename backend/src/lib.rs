pub mod database;
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
pub mod saved_routes_handlers;
pub mod terrain;

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
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

// Structures pour la sauvegarde/chargement de routes
#[derive(Debug, Serialize, Deserialize)]
pub struct SaveRouteRequest {
    pub name: String,
    pub route: RouteResponse,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedRouteInfo {
    pub filename: String,
    pub name: String,
    pub distance_km: f64,
    pub saved_at: String,
}

#[derive(Debug, Deserialize)]
pub struct LoadRouteQuery {
    pub filename: String,
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
        .route("/api/routes/list", get(list_routes_handler))
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
        .route("/api/routes/list", get(list_routes_handler))
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

// Handler pour sauvegarder une route sur le disque avec nom et timestamp
async fn save_route_handler(
    State(_state): State<AppState>,
    Json(req): Json<SaveRouteRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let save_dir = PathBuf::from("backend/data/saved_routes");

    // Créer le répertoire s'il n'existe pas (async I/O)
    if let Err(e) = tokio::fs::create_dir_all(&save_dir).await {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to create save directory: {}", e),
            }),
        ));
    }

    // Générer un nom de fichier unique avec timestamp
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let sanitized_name = req.name.replace([' ', '/'], "_");
    let filename = format!("{}_{}.json", timestamp, sanitized_name);
    let file_path = save_dir.join(&filename);

    // Créer les métadonnées
    let metadata = SavedRouteInfo {
        filename: filename.clone(),
        name: req.name.clone(),
        distance_km: req.route.distance_km,
        saved_at: chrono::Utc::now().to_rfc3339(),
    };

    // Créer une structure combinée pour sauvegarder
    let save_data = serde_json::json!({
        "metadata": metadata,
        "route": req.route
    });

    // Sérialiser et sauvegarder
    let json_str = serde_json::to_string_pretty(&save_data).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to serialize route: {}", e),
            }),
        )
    })?;

    tokio::fs::write(&file_path, json_str).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to write route file: {}", e),
            }),
        )
    })?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Route sauvegardée avec succès",
        "filename": filename
    })))
}

// Handler pour charger une route sauvegardée par nom de fichier
async fn load_route_handler(
    State(_state): State<AppState>,
    Query(query): Query<LoadRouteQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    // Validate filename to prevent path traversal attacks
    if query.filename.contains("..") || query.filename.contains('/') || query.filename.contains('\\') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                message: "Invalid filename: path traversal characters not allowed".to_string(),
            }),
        ));
    }

    let save_dir = PathBuf::from("backend/data/saved_routes");
    let file_path = save_dir.join(&query.filename);

    // Additional security: verify the resolved path is still within save_dir
    if let Ok(canonical_path) = file_path.canonicalize() {
        if let Ok(canonical_dir) = save_dir.canonicalize() {
            if !canonical_path.starts_with(canonical_dir) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        message: "Invalid filename: path escapes save directory".to_string(),
                    }),
                ));
            }
        }
    }

    // Vérifier si le fichier existe
    if !file_path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                message: format!("Route '{}' non trouvée", query.filename),
            }),
        ));
    }

    // Lire et désérialiser (async I/O)
    let json_str = tokio::fs::read_to_string(&file_path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to read route file: {}", e),
            }),
        )
    })?;

    let save_data: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to deserialize route: {}", e),
            }),
        )
    })?;

    // Extraire la route du format sauvegardé
    let route = save_data.get("route")
        .ok_or_else(|| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: "Invalid saved route format".to_string(),
            }),
        ))?;

    let route: RouteResponse = serde_json::from_value(route.clone()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to parse route: {}", e),
            }),
        )
    })?;

    Ok(Json(route))
}

// Handler pour lister toutes les routes sauvegardées
async fn list_routes_handler(
    State(_state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let save_dir = PathBuf::from("backend/data/saved_routes");

    // Créer le répertoire s'il n'existe pas (async I/O)
    if let Err(e) = tokio::fs::create_dir_all(&save_dir).await {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to create save directory: {}", e),
            }),
        ));
    }

    // Lire tous les fichiers JSON dans le répertoire (async I/O)
    let mut entries = tokio::fs::read_dir(&save_dir).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to read save directory: {}", e),
            }),
        )
    })?;

    let mut routes = Vec::new();

    while let Some(entry) = entries.next_entry().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                message: format!("Failed to read directory entry: {}", e),
            }),
        )
    })? {
        let path = entry.path();

        // Ne traiter que les fichiers JSON
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            // Lire le fichier (async I/O)
            if let Ok(json_str) = tokio::fs::read_to_string(&path).await {
                if let Ok(save_data) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    // Extraire les métadonnées
                    if let Some(metadata) = save_data.get("metadata") {
                        if let Ok(info) = serde_json::from_value::<SavedRouteInfo>(metadata.clone()) {
                            routes.push(info);
                        }
                    }
                }
            }
        }
    }

    // Trier par date (plus récent en premier)
    routes.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));

    Ok(Json(routes))
}
