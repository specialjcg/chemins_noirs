use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use backend::{
    database::Database,
    elevation::create_elevation_profile,
    engine::RouteEngine,
    graph::{BoundingBox, GraphBuilder, GraphBuilderConfig, GraphFile},
    loops::{self, LoopGenerationError},
    models::{ApiError, Coordinate, LoopRouteRequest, LoopRouteResponse, RouteRequest},
    partial_graph::PartialGraphConfig,
    routing::haversine_km,
    saved_routes_handlers,
};
use shared::MultiPointRouteRequest;
use shared::RouteResponse;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Handler for /api/route - generates partial graph on-demand and finds route
async fn route_handler(
    State(config): State<Arc<PartialGraphConfig>>,
    Json(req): Json<RouteRequest>,
) -> Result<Json<RouteResponse>, (StatusCode, String)> {
    tracing::info!("Route request: {:?} -> {:?}", req.start, req.end);

    // Calculate bounding box with margin for the route
    // Optimized: 5km margin as requested by user
    let bbox = BoundingBox::from_route(req.start, req.end, 5.0); // 5km margin

    // Use spawn_blocking to avoid blocking async runtime during graph generation
    let config_clone = config.clone();
    let graph = tokio::task::spawn_blocking(move || {
        prepare_graph_for_bbox(&config_clone, bbox)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Task spawn error: {}", e)))??;

    let engine = engine_from_graph(&config, &graph, "temp_route.json")?;

    tracing::info!("Engine created from partial graph");

    // Debug: Log graph stats
    tracing::info!(
        "Graph has {} nodes, {} edges",
        graph.nodes.len(),
        graph.edges.len()
    );

    match engine.find_path(&req) {
        Some(path) => {
            tracing::info!("Found path with {} waypoints", path.len());

            // Calculate distance
            let distance_km: f64 = path
                .windows(2)
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

            // Terrain mesh generation removed - MapLibre GL JS handles terrain rendering client-side
            let response = RouteResponse {
                path,
                distance_km,
                gpx_base64,
                metadata: None,
                elevation_profile,
                terrain: None,
            };

            Ok(Json(response))
        }
        None => {
            tracing::warn!(
                "No path found - graph has {} nodes, {} edges. Start: {:?}, End: {:?}",
                graph.nodes.len(),
                graph.edges.len(),
                req.start,
                req.end
            );
            Err((
                StatusCode::NOT_FOUND,
                "No route found - coordinates may be outside graph coverage or unreachable"
                    .to_string(),
            ))
        }
    }
}

async fn loop_route_handler(
    State(config): State<Arc<PartialGraphConfig>>,
    Json(req): Json<LoopRouteRequest>,
) -> Result<Json<LoopRouteResponse>, (StatusCode, String)> {
    tracing::info!(
        "Loop request from {:?} targeting {:.1} km",
        req.start,
        req.target_distance_km
    );

    let radius = (req.target_distance_km / 2.0).max(2.0) * 1.4 + req.distance_tolerance_km.max(1.0);
    let bbox = bbox_from_center(req.start, radius);

    // Use spawn_blocking for graph generation
    let config_clone = config.clone();
    let graph = tokio::task::spawn_blocking(move || {
        prepare_graph_for_bbox(&config_clone, bbox)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Task spawn error: {}", e)))??;

    let engine = engine_from_graph(&config, &graph, "temp_loop.json")?;

    match loops::generate_loops(&engine, &req).await {
        Ok(response) => Ok(Json(response)),
        Err(err) => {
            let status = match err {
                LoopGenerationError::InvalidTargetDistance => StatusCode::BAD_REQUEST,
                LoopGenerationError::NoLoopFound => StatusCode::NOT_FOUND,
                LoopGenerationError::Gpx(_) | LoopGenerationError::Elevation(_) => {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            };
            Err((status, err.to_string()))
        }
    }
}

/// Handler for /api/route/multi - optimized multi-waypoint routing with single graph generation
async fn multi_route_handler(
    State(config): State<Arc<PartialGraphConfig>>,
    Json(req): Json<MultiPointRouteRequest>,
) -> Result<Json<RouteResponse>, (StatusCode, String)> {
    if req.waypoints.len() < 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            "At least 2 waypoints required".to_string(),
        ));
    }

    tracing::info!(
        "Multi-point route request: {} waypoints, close_loop={}",
        req.waypoints.len(),
        req.close_loop
    );

    // Build waypoint list (add first point at end if closing loop)
    let mut points = req.waypoints.clone();
    if req.close_loop {
        points.push(req.waypoints[0]);
    }

    // Calculate bounding box that encompasses ALL waypoints
    let mut min_lat = f64::MAX;
    let mut max_lat = f64::MIN;
    let mut min_lon = f64::MAX;
    let mut max_lon = f64::MIN;

    for coord in &points {
        min_lat = min_lat.min(coord.lat);
        max_lat = max_lat.max(coord.lat);
        min_lon = min_lon.min(coord.lon);
        max_lon = max_lon.max(coord.lon);
    }

    // Add 5km margin around all points (optimized for user's use case)
    let margin_deg = 5.0 / 111.0; // ~5km in degrees
    let bbox = BoundingBox {
        min_lat: (min_lat - margin_deg).max(-90.0),
        max_lat: (max_lat + margin_deg).min(90.0),
        min_lon: (min_lon - margin_deg).clamp(-180.0, 180.0),
        max_lon: (max_lon + margin_deg).clamp(-180.0, 180.0),
    };

    // Validate bbox size to prevent DoS attacks
    if let Err(err_msg) = bbox.validate() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Invalid request: {}", err_msg),
        ));
    }

    tracing::info!("Generating single graph for bbox: {:?}", bbox);

    // Generate ONE graph for all segments using spawn_blocking
    let config_clone = config.clone();
    let graph = tokio::task::spawn_blocking(move || {
        prepare_graph_for_bbox(&config_clone, bbox)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Task spawn error: {}", e)))??;

    let engine = engine_from_graph(&config, &graph, "temp_multipoint.json")?;

    tracing::info!(
        "Engine created: {} nodes, {} edges",
        graph.nodes.len(),
        graph.edges.len()
    );

    // Now find path for each segment using the SAME engine
    let mut all_coords = Vec::new();
    let mut total_distance = 0.0;

    for i in 0..points.len() - 1 {
        let segment_req = RouteRequest {
            start: points[i],
            end: points[i + 1],
            w_pop: req.w_pop,
            w_paved: req.w_paved,
        };

        match engine.find_path(&segment_req) {
            Some(path) => {
                tracing::debug!(
                    "Segment {}/{}: {} waypoints",
                    i + 1,
                    points.len() - 1,
                    path.len()
                );

                // Merge paths, avoiding duplicate waypoints
                if i == 0 {
                    all_coords.extend(path.clone());
                } else {
                    all_coords.extend(path.into_iter().skip(1));
                }

                // Calculate segment distance
                let segment_distance: f64 = all_coords
                    .windows(2)
                    .map(|pair| haversine_km(pair[0], pair[1]))
                    .sum();
                total_distance = segment_distance;
            }
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    format!(
                        "No path found for segment {} -> {} (waypoints {}-{})",
                        i + 1,
                        i + 2,
                        points[i].lat,
                        points[i + 1].lat
                    ),
                ));
            }
        }
    }

    tracing::info!(
        "Multi-point route complete: {} total waypoints, {:.2}km",
        all_coords.len(),
        total_distance
    );

    // Fetch elevation profile for complete path
    let elevation_profile = match create_elevation_profile(&all_coords).await {
        Ok(profile) => {
            tracing::info!(
                "Elevation: min={:?}m, max={:?}m, ascent={:.0}m, descent={:.0}m",
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

    let response = RouteResponse {
        path: all_coords,
        distance_km: total_distance,
        gpx_base64: String::new(),
        metadata: None,
        elevation_profile,
        terrain: None,
    };

    Ok(Json(response))
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

    // Get PBF path, cache directory, and tiles directory from environment
    let pbf_path =
        std::env::var("PBF_PATH").unwrap_or_else(|_| "data/rhone-alpes-251111.osm.pbf".to_string());
    let cache_dir = std::env::var("CACHE_DIR").unwrap_or_else(|_| "data/cache".to_string());
    let tiles_dir = std::env::var("TILES_DIR").ok().map(PathBuf::from);

    tracing::info!(
        "Starting backend with on-demand graph generation from PBF: {}",
        pbf_path
    );
    tracing::info!("Cache directory: {}", cache_dir);

    if let Some(ref tiles_path) = tiles_dir {
        if tiles_path.exists() {
            tracing::info!("🚀 Tiles directory found: {} (FAST MODE enabled - <10s per route)", tiles_path.display());
        } else {
            tracing::warn!("⚠️  Tiles directory specified but not found: {}", tiles_path.display());
            tracing::warn!("   Run: cargo run --release --bin generate_tiles");
        }
    } else {
        tracing::info!("ℹ️  No tiles directory - using PBF mode (~2min first request)");
        tracing::info!("   To enable fast mode: export TILES_DIR=data/tiles");
        tracing::info!("   Then run: cargo run --release --bin generate_tiles");
    }

    // Create partial graph config
    let config = Arc::new(PartialGraphConfig {
        pbf_path: PathBuf::from(pbf_path),
        cache_dir: PathBuf::from(cache_dir),
        tiles_dir,
    });

    // Initialize PostgreSQL database
    let db = match Database::new().await {
        Ok(db) => {
            tracing::info!("✅ PostgreSQL connected successfully");

            // Run migrations
            if let Err(e) = db.migrate().await {
                tracing::error!("Failed to run migrations: {}", e);
                panic!("Database migration failed");
            }

            Arc::new(db)
        }
        Err(e) => {
            tracing::warn!("⚠️  PostgreSQL not available: {}", e);
            tracing::warn!("Set DATABASE_URL environment variable to enable saved routes.");
            tracing::warn!("Example: DATABASE_URL=postgresql://user:pass@localhost/chemins_noirs");
            panic!("Database required. See backend/DATABASE_SETUP.md for configuration.");
        }
    };

    // Create CORS layer to allow frontend requests
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Create router WITHOUT pre-loading any graph
    let app = axum::Router::new()
        .route(
            "/api/graph/partial",
            axum::routing::post(backend::partial_graph::partial_graph_handler),
        )
        .route("/api/loops", axum::routing::post(loop_route_handler))
        .route("/api/route", axum::routing::post(route_handler))
        .route("/api/route/multi", axum::routing::post(multi_route_handler))
        .route("/api/click_mode", axum::routing::get(click_mode_handler))
        .layer(cors.clone())
        .with_state(config)
        // Saved routes endpoints (PostgreSQL) - separate state
        .route("/api/routes", axum::routing::get(saved_routes_handlers::list_routes))
        .route("/api/routes", axum::routing::post(saved_routes_handlers::save_route))
        .route("/api/routes/:id", axum::routing::get(saved_routes_handlers::get_route))
        .route("/api/routes/:id", axum::routing::delete(saved_routes_handlers::delete_route))
        .route("/api/routes/:id/favorite", axum::routing::post(saved_routes_handlers::toggle_favorite))
        .layer(cors)
        .with_state(db);

    let addr: SocketAddr = "0.0.0.0:8080".parse().expect("valid socket address");
    tracing::info!("Starting backend on http://{addr}");
    tracing::info!("API endpoints:");
    tracing::info!("  POST /api/route - Find route with on-demand graph generation");
    tracing::info!("  POST /api/route/multi - Multi-waypoint route with single graph generation");
    tracing::info!("  POST /api/loops - Generate loop candidates");
    tracing::info!("  POST /api/graph/partial - Generate partial graph");
    tracing::info!("  GET /api/click_mode - Get click mode");
    tracing::info!("Saved routes (PostgreSQL):");
    tracing::info!("  POST /api/routes - Save route to database");
    tracing::info!("  GET /api/routes - List all saved routes");
    tracing::info!("  GET /api/routes/:id - Get specific route");
    tracing::info!("  DELETE /api/routes/:id - Delete route");
    tracing::info!("  POST /api/routes/:id/favorite - Toggle favorite");
    tracing::info!("Ready to generate graphs on-demand!");

    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();
}

/// Handler for /api/click_mode - returns a simple status
async fn click_mode_handler() -> &'static str {
    "RouteStart"
}

fn prepare_graph_for_bbox(
    config: &PartialGraphConfig,
    bbox: BoundingBox,
) -> Result<GraphFile, (StatusCode, String)> {
    let cache_key = bbox.cache_key();
    let cache_path = config.cache_dir.join(format!("{}.json", cache_key));

    // Check cache first
    if cache_path.exists() {
        tracing::info!("Loading cached partial graph: {}", cache_path.display());
        return GraphFile::read_from_path(&cache_path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load cache: {}", e),
            )
        });
    }

    // Try to use tiles if available (FAST - <10s)
    if let Some(tiles_dir) = &config.tiles_dir {
        if tiles_dir.exists() {
            tracing::info!("Using tile-based graph generation (fast mode)");
            let builder_config = GraphBuilderConfig { bbox: Some(bbox) };
            let builder = GraphBuilder::new(builder_config);

            match builder.build_from_tiles(tiles_dir, bbox) {
                Ok(graph) => {
                    // Cache the result
                    std::fs::create_dir_all(&config.cache_dir).ok();
                    graph.write_to_path(&cache_path).ok();
                    tracing::info!("Cached tile-based graph to: {}", cache_path.display());
                    return Ok(graph);
                }
                Err(e) => {
                    tracing::warn!("Tile-based generation failed ({}), falling back to PBF", e);
                }
            }
        }
    }

    // Fallback to PBF-based generation (SLOW - ~2min)
    tracing::info!("Generating partial graph from PBF for bbox: {:?}", bbox);
    let builder_config = GraphBuilderConfig { bbox: Some(bbox) };
    let builder = GraphBuilder::new(builder_config);
    let graph = builder.build_from_pbf(&config.pbf_path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to build graph: {}", e),
        )
    })?;
    std::fs::create_dir_all(&config.cache_dir).ok();
    graph.write_to_path(&cache_path).ok();
    tracing::info!("Cached partial graph to: {}", cache_path.display());
    Ok(graph)
}

fn engine_from_graph(
    _config: &PartialGraphConfig,
    graph: &GraphFile,
    _temp_name: &str,
) -> Result<RouteEngine, (StatusCode, String)> {
    // PERFORMANCE FIX: Create engine directly from GraphFile in memory
    // instead of writing to disk (slow for large graphs with millions of waypoints)
    // and reading it back. This eliminates multi-GB JSON serialization bottleneck.
    RouteEngine::from_graph_file(graph.clone()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create engine: {}", e),
        )
    })
}

fn bbox_from_center(center: Coordinate, radius_km: f64) -> BoundingBox {
    let lat_margin = radius_km / 111.0;
    let cos_lat = center.lat.to_radians().cos().abs().max(0.1);
    let lon_margin = radius_km / (111.0 * cos_lat);

    let bbox = BoundingBox {
        min_lat: (center.lat - lat_margin).max(-90.0),
        max_lat: (center.lat + lat_margin).min(90.0),
        min_lon: (center.lon - lon_margin).clamp(-180.0, 180.0),
        max_lon: (center.lon + lon_margin).clamp(-180.0, 180.0),
    };

    // Note: validation should be done by the caller if needed
    bbox
}
