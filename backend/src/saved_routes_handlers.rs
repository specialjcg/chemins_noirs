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
    };

    db.save_route(save_req)
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
