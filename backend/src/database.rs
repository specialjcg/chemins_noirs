// Module database - PostgreSQL connection pool and operations
// Architecture: Clean separation between data layer and business logic
// Principles: Functional, immutable, type-safe

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use shared::RouteResponse;
use sqlx::{postgres::PgPoolOptions, PgPool, FromRow};
use std::env;

/// Database error type
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Database connection error: {0}")]
    ConnectionError(#[from] sqlx::Error),

    #[error("Route not found: {0}")]
    NotFound(i32),

    #[error("Invalid route data: {0}")]
    InvalidData(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Saved route model (DB representation)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SavedRoute {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub distance_km: f32,
    pub total_ascent_m: Option<f32>,
    pub total_descent_m: Option<f32>,
    pub route_data: sqlx::types::JsonValue,
    pub gpx_data: Option<String>,
    pub is_favorite: bool,
    pub tags: Vec<String>,
    /// Original waypoints for multi-point routes (optional for backward compatibility)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub original_waypoints: Option<sqlx::types::JsonValue>,
}

/// Request to save a new route
#[derive(Debug, Serialize, Deserialize)]
pub struct SaveRouteRequest {
    pub name: String,
    pub description: Option<String>,
    pub route: RouteResponse,
    pub tags: Option<Vec<String>>,
    /// Original waypoints for multi-point routes (allows re-tracing with same waypoints)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub original_waypoints: Option<Vec<shared::Coordinate>>,
}

/// Database connection pool
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Create new database connection pool
    ///
    /// # Errors
    /// Returns DatabaseError if connection fails or DATABASE_URL is not set
    pub async fn new() -> Result<Self, DatabaseError> {
        let database_url = env::var("DATABASE_URL").map_err(|_| {
            DatabaseError::ConfigError(
                "DATABASE_URL environment variable not set".to_string(),
            )
        })?;

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await?;

        tracing::info!("PostgreSQL connection pool created");

        Ok(Self { pool })
    }

    /// Run database migrations
    ///
    /// # Errors
    /// Returns DatabaseError if migration fails
    pub async fn migrate(&self) -> Result<(), DatabaseError> {
        // Execute migration SQL directly using raw connection
        // SQLx query() cannot handle multiple statements, so we use a raw connection
        let mut conn = self.pool.acquire().await?;

        let migration_sql = include_str!("../migrations/20250128_create_saved_routes.sql");

        // Execute using raw SQL (supports multiple statements)
        sqlx::raw_sql(migration_sql)
            .execute(&mut *conn)
            .await?;

        tracing::info!("Database migrations completed");
        Ok(())
    }

    /// Save a new route
    ///
    /// # Arguments
    /// * `req` - SaveRouteRequest with route data
    ///
    /// # Returns
    /// The saved route with generated ID
    pub async fn save_route(&self, req: SaveRouteRequest) -> Result<SavedRoute, DatabaseError> {
        let route_json = serde_json::to_value(&req.route)
            .map_err(|e| DatabaseError::InvalidData(e.to_string()))?;

        let total_ascent = req.route.elevation_profile.as_ref()
            .map(|p| p.total_ascent);

        let total_descent = req.route.elevation_profile.as_ref()
            .map(|p| p.total_descent);

        // Convert original waypoints to JSON if present
        let original_waypoints_json = req.original_waypoints
            .map(|wp| serde_json::to_value(wp).ok())
            .flatten();

        let route = sqlx::query_as::<_, SavedRoute>(
            r#"
            INSERT INTO saved_routes (
                name, description, distance_km, total_ascent_m, total_descent_m,
                route_data, gpx_data, tags, original_waypoints
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *
            "#,
        )
        .bind(&req.name)
        .bind(&req.description)
        .bind(req.route.distance_km)
        .bind(total_ascent)
        .bind(total_descent)
        .bind(route_json)
        .bind(&req.route.gpx_base64)
        .bind(req.tags.unwrap_or_default())
        .bind(original_waypoints_json)
        .fetch_one(&self.pool)
        .await?;

        tracing::info!("Route saved: {} (ID: {})", route.name, route.id);
        Ok(route)
    }

    /// Get all saved routes (summary only)
    pub async fn list_routes(&self) -> Result<Vec<SavedRoute>, DatabaseError> {
        let routes = sqlx::query_as::<_, SavedRoute>(
            "SELECT * FROM saved_routes ORDER BY created_at DESC"
        )
        .fetch_all(&self.pool)
        .await?;

        tracing::info!("Retrieved {} routes", routes.len());
        Ok(routes)
    }

    /// Get a specific route by ID
    pub async fn get_route(&self, id: i32) -> Result<SavedRoute, DatabaseError> {
        let route = sqlx::query_as::<_, SavedRoute>(
            "SELECT * FROM saved_routes WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DatabaseError::NotFound(id))?;

        Ok(route)
    }

    /// Delete a route by ID
    pub async fn delete_route(&self, id: i32) -> Result<(), DatabaseError> {
        let result = sqlx::query("DELETE FROM saved_routes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(DatabaseError::NotFound(id));
        }

        tracing::info!("Route deleted: ID {}", id);
        Ok(())
    }

    /// Update route favorite status
    pub async fn toggle_favorite(&self, id: i32) -> Result<SavedRoute, DatabaseError> {
        let route = sqlx::query_as::<_, SavedRoute>(
            r#"
            UPDATE saved_routes
            SET is_favorite = NOT is_favorite
            WHERE id = $1
            RETURNING *
            "#
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DatabaseError::NotFound(id))?;

        tracing::info!("Route {} favorite status: {}", id, route.is_favorite);
        Ok(route)
    }

    /// Convert SavedRoute to RouteResponse
    pub fn to_route_response(saved: &SavedRoute) -> Result<RouteResponse, DatabaseError> {
        serde_json::from_value(saved.route_data.clone())
            .map_err(|e| DatabaseError::InvalidData(format!("Failed to deserialize route: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TDD: Tests will be implemented following RED-GREEN-REFACTOR cycle

    #[tokio::test]
    #[ignore] // Run with: cargo test -- --ignored
    async fn test_database_connection() {
        // RED: Write failing test first
        // GREEN: Implement minimum code to pass
        // REFACTOR: Improve without changing behavior

        let db = Database::new().await;
        assert!(db.is_ok(), "Database connection should succeed");
    }

    #[tokio::test]
    #[ignore]
    async fn test_save_and_retrieve_route() {
        // Test full cycle: save -> retrieve -> verify
        todo!("Implement TDD test")
    }

    #[tokio::test]
    #[ignore]
    async fn test_delete_route() {
        // Test delete operation with verification
        todo!("Implement TDD test")
    }
}
