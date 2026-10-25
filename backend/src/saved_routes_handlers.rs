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

use crate::database::{Database, DatabaseError, SaveRouteRequest, SavedRoute};
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
