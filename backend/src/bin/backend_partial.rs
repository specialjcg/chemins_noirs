use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{extract::State, http::StatusCode, Json};
use serde_json;
use backend::{
    elevation::create_elevation_profile,
    engine::RouteEngine,
    graph::{BoundingBox, GraphBuilder, GraphBuilderConfig},
    models::RouteRequest,
    partial_graph::PartialGraphConfig,
    routing::haversine_km,
};
use shared::RouteResponse;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Save route handler used by backend_partial to persist last route
async fn save_route_handler(
    Json(route): Json<RouteResponse>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let save_dir = PathBuf::from("backend/data/saved_routes");
    std::fs::create_dir_all(&save_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create save dir: {e}")))?;

    let file_path = save_dir.join("last_route.json");
    let json_str = serde_json::to_string_pretty(&route)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to serialize route: {e}")))?;

    std::fs::write(&file_path, json_str)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to write route file: {e}")))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Route sauvegardée avec succès"
    })))
}

/// Load route handler used by backend_partial to read last saved route
async fn load_route_handler() -> Result<Json<RouteResponse>, (StatusCode, String)> {
    let file_path = PathBuf::from("backend/data/saved_routes/last_route.json");
    if !file_path.exists() {
        return Err((StatusCode::NOT_FOUND, "Aucune route sauvegardée trouvée".to_string()));
    }

    let json_str = std::fs::read_to_string(&file_path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to read route file: {e}")))?;
    let route: RouteResponse = serde_json::from_str(&json_str)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to deserialize route: {e}")))?;

    Ok(Json(route))
}

/// Handler for /api/route - generates partial graph on-demand and finds route
async fn route_handler(
    State(config): State<Arc<PartialGraphConfig>>,
    Json(req): Json<RouteRequest>,
) -> Result<Json<RouteResponse>, (StatusCode, String)> {
    tracing::info!("Route request: {:?} -> {:?}", req.start, req.end);

    // Calculate bounding box with margin for the route
    let bbox = BoundingBox::from_route(req.start, req.end, 5.0); // 2km margin
    let cache_key = bbox.cache_key();
    let cache_path = config.cache_dir.join(format!("{}.json", cache_key));

    // Generate or load cached partial graph
    let graph = if cache_path.exists() {
        tracing::info!("Loading cached partial graph: {}", cache_path.display());
        backend::graph::GraphFile::read_from_path(&cache_path)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to load cache: {}", e)))?
    } else {
        tracing::info!("Generating partial graph for bbox: {:?}", bbox);
        let builder_config = GraphBuilderConfig { bbox: Some(bbox) };
        let builder = GraphBuilder::new(builder_config);
        let graph = builder.build_from_pbf(&config.pbf_path)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to build graph: {}", e)))?;

        // Cache for future requests
        std::fs::create_dir_all(&config.cache_dir).ok();
        graph.write_to_path(&cache_path).ok();
        tracing::info!("Cached partial graph to: {}", cache_path.display());
        graph
    };

    // Create temporary file for RouteEngine
    let temp_path = config.cache_dir.join("temp_route.json");
    graph.write_to_path(&temp_path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to write temp: {}", e)))?;

    // Load into RouteEngine and find path
    let engine = RouteEngine::from_file(&temp_path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create engine: {}", e)))?;

    tracing::info!("Engine created from partial graph");

    // Debug: Log graph stats
    tracing::info!("Graph has {} nodes, {} edges", graph.nodes.len(), graph.edges.len());

    match engine.find_path(&req) {
        Some(path) => {
            tracing::info!("Found path with {} waypoints", path.len());

            // Calculate distance
            let distance_km: f64 = path.windows(2)
                .map(|pair| haversine_km(pair[0], pair[1]))
                .sum();

            // Fetch elevation profile on-demand
            tracing::info!("Fetching elevation profile for {} points...", path.len());
            let elevation_profile = match create_elevation_profile(&path).await {
                Ok(profile) => {
                    tracing::info!(
                        "Elevation profile created: min={:?}m, max={:?}m, ascent={}m, descent={}m",
                        profile.min_elevation,
                        profile.max_elevation,
                        profile.total_ascent,
                        profile.total_descent
                    );
                    Some(profile)
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch elevation profile: {}", e);
                    None
                }
            };

            // For now, GPX base64 is empty - we can implement it later
            let gpx_base64 = String::new();

            let response = RouteResponse {
                path,
                distance_km,
                gpx_base64,
                metadata: None,
                elevation_profile,
            };

            Ok(Json(response))
        }
        None => {
            tracing::warn!("No path found - graph has {} nodes, {} edges. Start: {:?}, End: {:?}",
                graph.nodes.len(), graph.edges.len(), req.start, req.end);
            Err((StatusCode::NOT_FOUND, "No route found - coordinates may be outside graph coverage or unreachable".to_string()))
        }
    }
}

/// Backend binary that uses on-demand partial graph generation
/// instead of loading a massive graph file into memory
#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "backend=debug,axum::rejection=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Get PBF path and cache directory from environment
    let pbf_path = std::env::var("PBF_PATH")
        .unwrap_or_else(|_| "data/rhone-alpes-251111.osm.pbf".to_string());
    let cache_dir = std::env::var("CACHE_DIR")
        .unwrap_or_else(|_| "data/cache".to_string());

    tracing::info!(
        "Starting backend with on-demand graph generation from PBF: {}",
        pbf_path
    );
    tracing::info!("Cache directory: {}", cache_dir);

    // Create partial graph config
    let config = Arc::new(PartialGraphConfig {
        pbf_path: PathBuf::from(pbf_path),
        cache_dir: PathBuf::from(cache_dir),
    });

    // Create router WITHOUT pre-loading any graph
    let app = axum::Router::new()
        .route(
            "/api/graph/partial",
            axum::routing::post(backend::partial_graph::partial_graph_handler),
        )
        .route("/api/route", axum::routing::post(route_handler))
        .route("/api/routes/save", axum::routing::post(save_route_handler))
        .route("/api/routes/load", axum::routing::get(load_route_handler))
        .route("/api/click_mode", axum::routing::get(click_mode_handler))
        .with_state(config);

    let addr: SocketAddr = "0.0.0.0:8080".parse().expect("valid socket address");
    tracing::info!("Starting backend on http://{addr}");
    tracing::info!("API endpoints:");
    tracing::info!("  POST /api/route - Find route with on-demand graph generation");
    tracing::info!("  POST /api/graph/partial - Generate partial graph");
    tracing::info!("  POST /api/routes/save - Save route to disk");
    tracing::info!("  GET /api/routes/load - Load saved route from disk");
    tracing::info!("  GET /api/click_mode - Get click mode");
    tracing::info!("Ready to generate graphs on-demand!");

    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();
}

/// Handler for /api/click_mode - returns a simple status
async fn click_mode_handler() -> &'static str {
    "RouteStart"
}
