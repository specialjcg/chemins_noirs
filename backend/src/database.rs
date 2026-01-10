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
            .and_then(|wp| serde_json::to_value(wp).ok());

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
    use shared::{Coordinate, ElevationProfile};

    /// Helper to create test database with testcontainers
    /// Returns (Database, Container) - keep container alive to prevent Docker cleanup
    async fn setup_test_db() -> (Database, testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>) {
        use testcontainers::{runners::AsyncRunner, ImageExt};
        use testcontainers_modules::postgres::Postgres;

        // Start PostgreSQL container
        let container = Postgres::default()
            .with_tag("17-alpine")
            .start()
            .await
            .expect("Failed to start PostgreSQL container");

        // Build connection string
        let host = container.get_host().await.expect("Failed to get host");
        let port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("Failed to get port");
        let database_url = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            host, port
        );

        // Set environment variable for Database::new()
        std::env::set_var("DATABASE_URL", &database_url);

        // Create database connection
        let db = Database::new().await.expect("Failed to connect to test DB");

        // Run migrations
        db.migrate().await.expect("Failed to run migrations");

        (db, container)
    }

    /// Create a sample route for testing
    fn create_test_route_request(name: &str) -> SaveRouteRequest {
        let path = vec![
            Coordinate { lat: 45.0, lon: 5.0 },
            Coordinate {
                lat: 45.01,
                lon: 5.01,
            },
            Coordinate {
                lat: 45.02,
                lon: 5.02,
            },
        ];

        let route = RouteResponse {
            path: path.clone(),
            distance_km: 2.5,
            gpx_base64: "R1BYIG1vY2sgZGF0YQ==".to_string(), // "GPX mock data" in base64
            metadata: None,
            elevation_profile: Some(ElevationProfile {
                elevations: vec![Some(300.0), Some(350.0), Some(400.0)],
                min_elevation: Some(300.0),
                max_elevation: Some(400.0),
                total_ascent: 100.0,
                total_descent: 0.0,
            }),
            terrain: None,
        };

        SaveRouteRequest {
            name: name.to_string(),
            description: Some("Test route description".to_string()),
            route,
            tags: Some(vec!["test".to_string(), "scenic".to_string()]),
            original_waypoints: Some(path),
        }
    }

    #[tokio::test]
    async fn test_database_connection() {
        let (db, _container) = setup_test_db().await;
        // If we got here, connection succeeded
        assert!(db.pool.acquire().await.is_ok());
    }

    #[tokio::test]
    async fn test_save_route() {
        let (db, _container) = setup_test_db().await;
        let request = create_test_route_request("Test Route");

        let saved = db.save_route(request).await.expect("Failed to save route");

        assert!(saved.id > 0);
        assert_eq!(saved.name, "Test Route");
        assert_eq!(saved.description, Some("Test route description".to_string()));
        assert_eq!(saved.distance_km, 2.5);
        assert_eq!(saved.total_ascent_m, Some(100.0));
        assert_eq!(saved.total_descent_m, Some(0.0));
        assert_eq!(saved.tags, vec!["test", "scenic"]);
        assert!(!saved.is_favorite); // Default value
    }

    #[tokio::test]
    async fn test_save_and_retrieve_route() {
        let (db, _container) = setup_test_db().await;
        let request = create_test_route_request("Retrieve Test");

        // Save route
        let saved = db.save_route(request).await.expect("Failed to save route");
        let route_id = saved.id;

        // Retrieve route
        let retrieved = db
            .get_route(route_id)
            .await
            .expect("Failed to retrieve route");

        assert_eq!(retrieved.id, route_id);
        assert_eq!(retrieved.name, "Retrieve Test");
        assert_eq!(retrieved.distance_km, 2.5);
        assert_eq!(retrieved.total_ascent_m, Some(100.0));

        // Verify we can deserialize route data
        let route_response = Database::to_route_response(&retrieved)
            .expect("Failed to convert to RouteResponse");
        assert_eq!(route_response.path.len(), 3);
        assert_eq!(route_response.distance_km, 2.5);
    }

    #[tokio::test]
    async fn test_list_routes() {
        let (db, _container) = setup_test_db().await;

        // Save multiple routes
        db.save_route(create_test_route_request("Route 1"))
            .await
            .expect("Failed to save route 1");
        db.save_route(create_test_route_request("Route 2"))
            .await
            .expect("Failed to save route 2");
        db.save_route(create_test_route_request("Route 3"))
            .await
            .expect("Failed to save route 3");

        // List all routes
        let routes = db.list_routes().await.expect("Failed to list routes");

        assert_eq!(routes.len(), 3);
        // Routes are ordered by created_at DESC, so most recent first
        assert_eq!(routes[0].name, "Route 3");
        assert_eq!(routes[1].name, "Route 2");
        assert_eq!(routes[2].name, "Route 1");
    }

    #[tokio::test]
    async fn test_delete_route() {
        let (db, _container) = setup_test_db().await;

        // Save a route
        let saved = db
            .save_route(create_test_route_request("To Delete"))
            .await
            .expect("Failed to save route");
        let route_id = saved.id;

        // Verify it exists
        assert!(db.get_route(route_id).await.is_ok());

        // Delete it
        db.delete_route(route_id)
            .await
            .expect("Failed to delete route");

        // Verify it no longer exists
        let result = db.get_route(route_id).await;
        assert!(matches!(result, Err(DatabaseError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_delete_nonexistent_route() {
        let (db, _container) = setup_test_db().await;

        // Try to delete a route that doesn't exist
        let result = db.delete_route(9999).await;
        assert!(matches!(result, Err(DatabaseError::NotFound(9999))));
    }

    #[tokio::test]
    async fn test_toggle_favorite() {
        let (db, _container) = setup_test_db().await;

        // Save a route
        let saved = db
            .save_route(create_test_route_request("Favorite Test"))
            .await
            .expect("Failed to save route");
        let route_id = saved.id;

        // Initially not favorite
        assert!(!saved.is_favorite);

        // Toggle to favorite
        let toggled = db
            .toggle_favorite(route_id)
            .await
            .expect("Failed to toggle favorite");
        assert!(toggled.is_favorite);

        // Toggle back to not favorite
        let toggled_again = db
            .toggle_favorite(route_id)
            .await
            .expect("Failed to toggle favorite again");
        assert!(!toggled_again.is_favorite);
    }

    #[tokio::test]
    async fn test_save_route_without_optional_fields() {
        let (db, _container) = setup_test_db().await;

        let route = RouteResponse {
            path: vec![
                Coordinate { lat: 45.0, lon: 5.0 },
                Coordinate {
                    lat: 45.01,
                    lon: 5.01,
                },
            ],
            distance_km: 1.5,
            gpx_base64: "bW9jaw==".to_string(),
            metadata: None,
            elevation_profile: None, // No elevation data
            terrain: None,
        };

        let request = SaveRouteRequest {
            name: "Minimal Route".to_string(),
            description: None, // No description
            route,
            tags: None, // No tags
            original_waypoints: None, // No waypoints
        };

        let saved = db.save_route(request).await.expect("Failed to save route");

        assert_eq!(saved.name, "Minimal Route");
        assert_eq!(saved.description, None);
        assert_eq!(saved.total_ascent_m, None);
        assert_eq!(saved.total_descent_m, None);
        assert_eq!(saved.tags.len(), 0);
        assert_eq!(saved.original_waypoints, None);
    }

    #[tokio::test]
    async fn test_get_nonexistent_route() {
        let (db, _container) = setup_test_db().await;

        let result = db.get_route(12345).await;
        assert!(matches!(result, Err(DatabaseError::NotFound(12345))));
    }

    #[tokio::test]
    async fn test_list_routes_empty() {
        let (db, _container) = setup_test_db().await;

        let routes = db.list_routes().await.expect("Failed to list routes");
        assert_eq!(routes.len(), 0);
    }
}
