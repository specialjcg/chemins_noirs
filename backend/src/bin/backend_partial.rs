use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{extract::State, http::StatusCode, Json};
use backend::{
    database::Database,
    elevation::create_elevation_profile,
    engine::RouteEngine,
    graph::{BoundingBox, GraphBuilder, GraphBuilderConfig, GraphFile},
    loops::{self, LoopGenerationError},
    models::{Coordinate, LoopRouteRequest, LoopRouteResponse, RouteRequest},
    partial_graph::PartialGraphConfig,
    poi,
    routing::{estimate_time_minutes, haversine_km, rate_difficulty},
    saved_routes_handlers,
};
use shared::MultiPointRouteRequest;
use shared::RouteResponse;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// In-memory engine cache: keeps the RouteEngine + its bbox so subsequent
/// requests that fall within the same coverage area skip PBF parsing (~7-11s)
/// and engine creation (~1-2s). Uses Arc to avoid cloning the large engine.
struct CachedEngine {
    engine: Arc<RouteEngine>,
    bbox: BoundingBox,
}

static ENGINE_CACHE: std::sync::LazyLock<tokio::sync::RwLock<Option<CachedEngine>>> =
    std::sync::LazyLock::new(|| tokio::sync::RwLock::new(None));

/// Build or reuse an engine for the given bbox.
/// On cache miss, builds with generous padding so nearby future requests hit.
async fn get_or_build_engine(
    config: &Arc<PartialGraphConfig>,
    needed_bbox: BoundingBox,
) -> Result<Arc<RouteEngine>, (StatusCode, String)> {
    // Fast path: check if cached engine covers the needed bbox
    {
        let guard = ENGINE_CACHE.read().await;
        if let Some(cached) = guard.as_ref() {
            if cached.bbox.contains_bbox(&needed_bbox) {
                tracing::info!("PERF ENGINE_CACHE HIT — reusing in-memory engine");
                return Ok(Arc::clone(&cached.engine));
            }
        }
    }

    // Cache miss: build graph with generous bbox (2x padding)
    let padded_bbox = pad_bbox(&needed_bbox);
    tracing::info!("PERF ENGINE_CACHE MISS — building engine for padded bbox: {:?}", padded_bbox);

    let t_graph = std::time::Instant::now();
    let config_clone = config.clone();
    let graph = tokio::task::spawn_blocking(move || {
        prepare_graph_for_bbox(&config_clone, padded_bbox)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Task spawn error: {}", e)))??;
    tracing::info!(
        "PERF graph: {:.0}ms ({} nodes, {} edges)",
        t_graph.elapsed().as_secs_f64() * 1000.0,
        graph.nodes.len(),
        graph.edges.len()
    );

    let t_engine = std::time::Instant::now();
    let engine = RouteEngine::from_graph_file(graph).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create engine: {}", e),
        )
    })?;
    tracing::info!("PERF engine: {:.0}ms", t_engine.elapsed().as_secs_f64() * 1000.0);

    let engine = Arc::new(engine);

    // Store in cache
    {
        let mut guard = ENGINE_CACHE.write().await;
        *guard = Some(CachedEngine {
            engine: Arc::clone(&engine),
            bbox: padded_bbox,
        });
        tracing::info!("PERF ENGINE_CACHE stored (padded bbox: {:?})", padded_bbox);
    }

    Ok(engine)
}

/// Pad a bbox proportionally to the route spread: 20% of span, min 2km per side.
/// Small routes (nearby clicks) → small padding. Large routes → larger padding.
fn pad_bbox(bbox: &BoundingBox) -> BoundingBox {
    let lat_span = bbox.max_lat - bbox.min_lat;
    let lon_span = bbox.max_lon - bbox.min_lon;

    let min_pad_km = 2.0;
    let lat_pad = (lat_span * 0.2).max(min_pad_km / 111.0);

    let avg_lat = (bbox.min_lat + bbox.max_lat) / 2.0;
    let cos_lat = avg_lat.to_radians().cos().abs().max(0.1);
    let lon_pad = (lon_span * 0.2).max(min_pad_km / (111.0 * cos_lat));

    BoundingBox {
        min_lat: (bbox.min_lat - lat_pad).max(-90.0),
        max_lat: (bbox.max_lat + lat_pad).min(90.0),
        min_lon: (bbox.min_lon - lon_pad).clamp(-180.0, 180.0),
        max_lon: (bbox.max_lon + lon_pad).clamp(-180.0, 180.0),
    }
}

/// Handler for /api/route - generates partial graph on-demand and finds route
async fn route_handler(
    State(config): State<Arc<PartialGraphConfig>>,
    Json(req): Json<RouteRequest>,
) -> Result<Json<RouteResponse>, (StatusCode, String)> {
    let t_total = std::time::Instant::now();
    tracing::info!("Route request: {:?} -> {:?}", req.start, req.end);

    // Calculate bounding box with margin for the route
    let bbox = BoundingBox::from_route(req.start, req.end, 5.0);

    let engine = get_or_build_engine(&config, bbox).await?;

    let t_path = std::time::Instant::now();
    match engine.find_path(&req) {
        Some(path) => {
            tracing::info!("PERF pathfinding: {:.0}ms ({} points)", t_path.elapsed().as_secs_f64() * 1000.0, path.len());

            // Calculate distance
            let distance_km: f64 = path
                .windows(2)
                .map(|pair| haversine_km(pair[0], pair[1]))
                .sum();

            // Fetch elevation profile on-demand
            let t_elev = std::time::Instant::now();
            let elevation_profile = match create_elevation_profile(&path).await {
                Ok(profile) => {
                    tracing::info!(
                        "PERF elevation: {:.0}ms (ascent={:.0}m, descent={:.0}m)",
                        t_elev.elapsed().as_secs_f64() * 1000.0,
                        profile.total_ascent,
                        profile.total_descent
                    );
                    Some(profile)
                }
                Err(e) => {
                    tracing::warn!("PERF elevation: {:.0}ms (FAILED: {})", t_elev.elapsed().as_secs_f64() * 1000.0, e);
                    None
                }
            };

            // For now, GPX base64 is empty - we can implement it later
            let gpx_base64 = String::new();

            // Compute analytics from elevation profile
            let (estimated_time, difficulty) = match &elevation_profile {
                Some(profile) => {
                    let time = estimate_time_minutes(distance_km, profile.total_ascent);
                    let diff = rate_difficulty(&profile.elevations, &path, profile.total_ascent);
                    (Some(time), Some(diff))
                }
                None => (None, None),
            };

            let response = RouteResponse {
                path,
                distance_km,
                gpx_base64,
                metadata: None,
                elevation_profile,
                terrain: None,
                snapped_waypoints: None,
                estimated_time_minutes: estimated_time,
                difficulty,
                surface_breakdown: None,
                segments: None,
            };

            tracing::info!("PERF TOTAL /api/route: {:.0}ms ({:.2}km)", t_total.elapsed().as_secs_f64() * 1000.0, distance_km);
            Ok(Json(response))
        }
        None => {
            tracing::warn!(
                "No path found. Start: {:?}, End: {:?}",
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
    let t_total = std::time::Instant::now();
    tracing::info!(
        "Loop request from {:?} targeting {:.1} km",
        req.start,
        req.target_distance_km
    );

    let radius = (req.target_distance_km / 2.0).max(2.0) * 1.4 + req.distance_tolerance_km.max(1.0);
    let bbox = bbox_from_center(req.start, radius);

    let engine = get_or_build_engine(&config, bbox).await?;

    let t_loops = std::time::Instant::now();
    match loops::generate_loops(&engine, &req).await {
        Ok(response) => {
            tracing::info!("PERF loops: {:.0}ms ({} candidates)", t_loops.elapsed().as_secs_f64() * 1000.0, response.candidates.len());
            tracing::info!("PERF TOTAL /api/loops: {:.0}ms", t_total.elapsed().as_secs_f64() * 1000.0);
            Ok(Json(response))
        }
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
    let t_total = std::time::Instant::now();
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

    let engine = get_or_build_engine(&config, bbox).await?;

    // Helper: push coordinate only if it differs from the last one (dedup)
    let push_dedup = |coords: &mut Vec<Coordinate>, c: Coordinate| {
        if coords
            .last()
            .map_or(true, |last| {
                (last.lat - c.lat).abs() > 1e-7 || (last.lon - c.lon).abs() > 1e-7
            })
        {
            coords.push(c);
        }
    };

    // Now find path for each segment using the SAME engine.
    // Path only contains road-snapped coordinates (no off-road spikes to click positions).
    // We also collect the snapped waypoint positions (on-road projections) so the
    // frontend can place markers exactly on the route line.
    let mut all_coords: Vec<Coordinate> = Vec::new();
    let mut snapped_waypoints: Vec<Coordinate> = Vec::new();
    let mut total_distance = 0.0;
    // Track segment boundaries: (start_idx, end_idx) in all_coords
    let mut segment_boundaries: Vec<(usize, usize)> = Vec::new();

    let t_pathfinding = std::time::Instant::now();
    for i in 0..points.len() - 1 {
        let segment_req = RouteRequest {
            start: points[i],
            end: points[i + 1],
            w_pop: req.w_pop,
            w_paved: req.w_paved,
        };

        let t_seg = std::time::Instant::now();
        match engine.find_path(&segment_req) {
            Some(path) => {
                tracing::info!(
                    "PERF segment {}/{}: {:.0}ms ({} pts)",
                    i + 1,
                    points.len() - 1,
                    t_seg.elapsed().as_secs_f64() * 1000.0,
                    path.len()
                );

                // Collect snapped positions: path starts at snap(points[i]),
                // ends at snap(points[i+1])
                if i == 0 {
                    snapped_waypoints.push(path[0]);
                }
                if let Some(&last) = path.last() {
                    snapped_waypoints.push(last);
                }

                // Record start index for this segment
                let actual_start = if all_coords.is_empty() { 0 } else { all_coords.len() - 1 };

                // Add the routed path (dedup avoids duplicate at segment boundaries)
                for &coord in &path {
                    push_dedup(&mut all_coords, coord);
                }

                let end_idx = all_coords.len() - 1;
                segment_boundaries.push((if i == 0 { 0 } else { actual_start }, end_idx));

                // Calculate total distance so far
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

    // Only keep snapped positions for original waypoints (exclude close_loop duplicate)
    snapped_waypoints.truncate(req.waypoints.len());

    tracing::info!(
        "PERF pathfinding total: {:.0}ms ({} segments, {} pts, {:.2}km)",
        t_pathfinding.elapsed().as_secs_f64() * 1000.0,
        points.len() - 1,
        all_coords.len(),
        total_distance
    );

    // Fetch elevation profile for complete path
    let t_elev = std::time::Instant::now();
    let elevation_profile = match create_elevation_profile(&all_coords).await {
        Ok(profile) => {
            tracing::info!(
                "PERF elevation: {:.0}ms (ascent={:.0}m, descent={:.0}m)",
                t_elev.elapsed().as_secs_f64() * 1000.0,
                profile.total_ascent,
                profile.total_descent
            );
            Some(profile)
        }
        Err(e) => {
            tracing::warn!("PERF elevation: {:.0}ms (FAILED: {})", t_elev.elapsed().as_secs_f64() * 1000.0, e);
            None
        }
    };

    let (estimated_time, difficulty) = match &elevation_profile {
        Some(profile) => {
            let time = estimate_time_minutes(total_distance, profile.total_ascent);
            let diff = rate_difficulty(&profile.elevations, &all_coords, profile.total_ascent);
            (Some(time), Some(diff))
        }
        None => (None, None),
    };

    // Compute per-segment statistics
    let segments = if segment_boundaries.len() >= 2 {
        let seg_stats: Vec<shared::SegmentStats> = segment_boundaries
            .iter()
            .map(|&(from_idx, to_idx)| {
                // Distance for this segment
                let seg_dist: f64 = all_coords[from_idx..=to_idx]
                    .windows(2)
                    .map(|pair| haversine_km(pair[0], pair[1]))
                    .sum();

                // Elevation stats for this segment
                let (ascent, descent) = match &elevation_profile {
                    Some(profile) => {
                        let mut asc = 0.0_f64;
                        let mut desc = 0.0_f64;
                        let elevs = &profile.elevations;
                        for j in from_idx..to_idx {
                            if j + 1 < elevs.len() {
                                if let (Some(e1), Some(e2)) = (elevs[j], elevs[j + 1]) {
                                    let diff = e2 - e1;
                                    if diff > 0.0 {
                                        asc += diff;
                                    } else {
                                        desc += diff.abs();
                                    }
                                }
                            }
                        }
                        (asc, desc)
                    }
                    None => (0.0, 0.0),
                };

                let avg_slope = if seg_dist > 0.001 {
                    (ascent - descent) / (seg_dist * 1000.0) * 100.0
                } else {
                    0.0
                };

                shared::SegmentStats {
                    from_index: from_idx,
                    to_index: to_idx,
                    distance_km: (seg_dist * 100.0).round() / 100.0,
                    ascent_m: ascent.round(),
                    descent_m: descent.round(),
                    avg_slope_pct: (avg_slope * 10.0).round() / 10.0,
                }
            })
            .collect();
        Some(seg_stats)
    } else {
        None
    };

    let response = RouteResponse {
        path: all_coords,
        distance_km: total_distance,
        gpx_base64: String::new(),
        metadata: None,
        elevation_profile,
        terrain: None,
        snapped_waypoints: Some(snapped_waypoints),
        estimated_time_minutes: estimated_time,
        difficulty,
        surface_breakdown: None,
        segments,
    };

    tracing::info!("PERF TOTAL /api/route/multi: {:.0}ms ({} wps, {:.2}km)", t_total.elapsed().as_secs_f64() * 1000.0, req.waypoints.len(), total_distance);
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
    let tiles_dir = std::env::var("TILES_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            let default = PathBuf::from("data/tiles");
            if default.exists() {
                Some(default)
            } else {
                None
            }
        });

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
        .route("/api/pois", axum::routing::get(pois_handler))
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

    let addr: SocketAddr = "0.0.0.0:8090".parse().expect("valid socket address");
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

#[derive(serde::Deserialize)]
struct PoiQuery {
    min_lat: f64,
    max_lat: f64,
    min_lon: f64,
    max_lon: f64,
}

/// Handler for /api/pois?min_lat=&max_lat=&min_lon=&max_lon=
async fn pois_handler(
    State(config): State<Arc<PartialGraphConfig>>,
    axum::extract::Query(query): axum::extract::Query<PoiQuery>,
) -> Result<Json<Vec<poi::Poi>>, (StatusCode, String)> {
    let bbox = BoundingBox {
        min_lat: query.min_lat,
        max_lat: query.max_lat,
        min_lon: query.min_lon,
        max_lon: query.max_lon,
    };

    let pbf_path = config.pbf_path.clone();
    let pois = tokio::task::spawn_blocking(move || {
        poi::extract_pois_from_pbf(&pbf_path, bbox)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Task error: {}", e)))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(pois))
}

fn prepare_graph_for_bbox(
    config: &PartialGraphConfig,
    bbox: BoundingBox,
) -> Result<GraphFile, (StatusCode, String)> {
    let t0 = std::time::Instant::now();
    let cache_key = bbox.cache_key();
    let cache_path = config.cache_dir.join(format!("{}.bin", cache_key));

    // Check cache first
    if cache_path.exists() {
        let result = GraphFile::read_from_path(&cache_path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load cache: {}", e),
            )
        });
        tracing::info!("PERF prepare_graph CACHE HIT: {:.0}ms ({})", t0.elapsed().as_secs_f64() * 1000.0, cache_path.display());
        return result;
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
                    tracing::info!("PERF prepare_graph TILES: {:.0}ms", t0.elapsed().as_secs_f64() * 1000.0);
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
    tracing::info!("PERF prepare_graph PBF: {:.0}ms", t0.elapsed().as_secs_f64() * 1000.0);
    Ok(graph)
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
