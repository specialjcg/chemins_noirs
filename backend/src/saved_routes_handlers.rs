// Handlers for saved routes API endpoints
// Architecture: RESTful API with PostgreSQL backend
// Principles: Functional, immutable, type-safe

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::database::{Database, DatabaseError, SaveRouteRequest, SavedRoute};
use crate::gpx_export::encode_route_as_gpx_with_elevations;
use crate::models::ApiError;
use shared::RouteResponse;

/// Request to save a route with metadata
#[derive(Debug, Deserialize)]
pub struct SaveRouteApiRequest {
    pub name: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub original_waypoints: Option<Vec<shared::Coordinate>>,
    #[serde(default)]
    pub folder: Option<String>,
}

/// Body of `PATCH /api/routes/:id/folder`; `null` or blank removes the folder
#[derive(Debug, Deserialize)]
pub struct SetFolderRequest {
    pub folder: Option<String>,
}

/// POST /api/routes - Save a new route
pub async fn save_route(
    State(db): State<Arc<Database>>,
    Json(payload): Json<(SaveRouteApiRequest, RouteResponse)>,
) -> Result<Json<SavedRoute>, (StatusCode, Json<ApiError>)> {
    let (req, route) = payload;

    let save_req = SaveRouteRequest {
        name: req.name,
        description: req.description,
        route,
        tags: req.tags,
        original_waypoints: req.original_waypoints,
        folder: req.folder,
    };

    db.save_route(save_req)
        .await
        .map(Json)
        .map_err(db_error_to_api_error)
}

/// PUT /api/routes/:id — Remplacer un itinéraire existant
///
/// Même corps que `save_route`, mais écrit sur une ligne déjà là. Sert à
/// reprendre un itinéraire chargé dont on a déplacé les points : sans cela
/// chaque retouche créait une ligne de plus, et il fallait supprimer
/// l'ancienne à la main.
pub async fn update_route(
    State(db): State<Arc<Database>>,
    Path(id): Path<i32>,
    Json(payload): Json<(SaveRouteApiRequest, RouteResponse)>,
) -> Result<Json<SavedRoute>, (StatusCode, Json<ApiError>)> {
    let (req, route) = payload;

    let save_req = SaveRouteRequest {
        name: req.name,
        description: req.description,
        route,
        tags: req.tags,
        original_waypoints: req.original_waypoints,
        folder: req.folder,
    };

    db.update_route(id, save_req)
        .await
        .map(Json)
        .map_err(db_error_to_api_error)
}

/// GET /api/routes - List all saved routes
pub async fn list_routes(
    State(db): State<Arc<Database>>,
) -> Result<Json<Vec<SavedRoute>>, (StatusCode, Json<ApiError>)> {
    db.list_routes()
        .await
        .map(Json)
        .map_err(db_error_to_api_error)
}

/// GET /api/routes/:id - Get a specific route (returns full SavedRoute with metadata)
pub async fn get_route(
    State(db): State<Arc<Database>>,
    Path(id): Path<i32>,
) -> Result<Json<SavedRoute>, (StatusCode, Json<ApiError>)> {
    db.get_route(id)
        .await
        .map(Json)
        .map_err(db_error_to_api_error)
}

/// DELETE /api/routes/:id - Delete a route
pub async fn delete_route(
    State(db): State<Arc<Database>>,
    Path(id): Path<i32>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    db.delete_route(id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(db_error_to_api_error)
}

/// DELETE /api/folders/:name - Delete every route of a folder
pub async fn delete_folder(
    State(db): State<Arc<Database>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    db.delete_folder(&name)
        .await
        .map(|deleted| Json(serde_json::json!({ "deleted": deleted })))
        .map_err(db_error_to_api_error)
}

/// POST /api/routes/:id/favorite - Toggle favorite status
pub async fn toggle_favorite(
    State(db): State<Arc<Database>>,
    Path(id): Path<i32>,
) -> Result<Json<SavedRoute>, (StatusCode, Json<ApiError>)> {
    db.toggle_favorite(id)
        .await
        .map(Json)
        .map_err(db_error_to_api_error)
}

/// PATCH /api/routes/:id/folder - Move a route to a folder
pub async fn set_folder(
    State(db): State<Arc<Database>>,
    Path(id): Path<i32>,
    Json(req): Json<SetFolderRequest>,
) -> Result<Json<SavedRoute>, (StatusCode, Json<ApiError>)> {
    db.set_folder(id, req.folder)
        .await
        .map(Json)
        .map_err(db_error_to_api_error)
}

/// Directory of the Garmin CLI (`experiments/hardware/garmin`): the binary is
/// `target/release/garmin` and must run there, where its token and `.env` live.
const DEFAULT_GARMIN_CLI_DIR: &str = "/home/jcgouleau/Projects/experiments/hardware/garmin";

/// POST /api/routes/:id/garmin - Create a Garmin Connect course from a saved
/// route and send it to the watch, through the Garmin CLI.
pub async fn send_to_garmin(
    State(db): State<Arc<Database>>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let saved = db.get_route(id).await.map_err(db_error_to_api_error)?;
    let route = Database::to_route_response(&saved).map_err(db_error_to_api_error)?;
    let elevations = route.elevation_profile.as_ref().map(|p| p.elevations.as_slice());
    let gpx = encode_route_as_gpx_with_elevations(&route.path, elevations)
        .ok()
        .and_then(|b64| BASE64.decode(b64).ok())
        .ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Génération du GPX impossible"))?;

    let name = saved.name.clone();
    let message = tokio::task::spawn_blocking(move || garmin_push(id, &name, &gpx))
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
        .map_err(|e| api_error(StatusCode::BAD_GATEWAY, &e))?;

    Ok(Json(serde_json::json!({ "message": message })))
}

/// Runs `garmin course push <gpx> --name <name>` and returns its last output line.
fn garmin_push(id: i32, name: &str, gpx: &[u8]) -> Result<String, String> {
    let cli_dir = std::env::var("GARMIN_CLI_DIR").unwrap_or_else(|_| DEFAULT_GARMIN_CLI_DIR.into());
    let cli_dir = std::path::Path::new(&cli_dir);
    let gpx_path = std::env::temp_dir().join(format!("chemins-noirs-garmin-{id}-{}.gpx", std::process::id()));
    std::fs::write(&gpx_path, gpx).map_err(|e| format!("écriture du GPX : {e}"))?;

    let output = std::process::Command::new(cli_dir.join("target/release/garmin"))
        .current_dir(cli_dir)
        .arg("course")
        .arg("push")
        .arg(&gpx_path)
        .arg("--name")
        .arg(name)
        .output();
    let _ = std::fs::remove_file(&gpx_path);

    let output = output.map_err(|e| format!("outil Garmin introuvable dans {} : {e}", cli_dir.display()))?;
    let last_line = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_owned()
    };
    if output.status.success() {
        Ok(last_line(&output.stdout))
    } else {
        Err(format!("Envoi Garmin échoué : {}", last_line(&output.stderr)))
    }
}

fn api_error(status: StatusCode, message: &str) -> (StatusCode, Json<ApiError>) {
    (status, Json(ApiError { message: message.to_owned() }))
}

/// Convert DatabaseError to API error response
fn db_error_to_api_error(err: DatabaseError) -> (StatusCode, Json<ApiError>) {
    let (status, message) = match err {
        DatabaseError::NotFound(id) => (
            StatusCode::NOT_FOUND,
            format!("Route with ID {} not found", id),
        ),
        DatabaseError::InvalidData(msg) => (StatusCode::BAD_REQUEST, msg),
        DatabaseError::ConfigError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        DatabaseError::ConnectionError(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Database connection error: {}", e),
        ),
    };

    (status, Json(ApiError { message }))
}

#[cfg(test)]
mod tests {
    

    // TDD: Integration tests with test database

    #[tokio::test]
    #[ignore]
    async fn test_save_and_retrieve_route_api() {
        // Test full API cycle
        todo!("Implement integration test with test database")
    }

    #[tokio::test]
    #[ignore]
    async fn test_list_routes_empty() {
        // Test list with no routes
        todo!("Implement test")
    }

    #[tokio::test]
    #[ignore]
    async fn test_delete_nonexistent_route() {
        // Test 404 error handling
        todo!("Implement test")
    }
}
