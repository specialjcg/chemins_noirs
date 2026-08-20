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

/// Handler for /api/plan-stages — compute optimal daily stages from lodgings
async fn plan_stages_handler(
    Json(req): Json<backend::lodgings::PlanStagesRequest>,
) -> Result<Json<backend::lodgings::PlanStagesResponse>, (StatusCode, String)> {
    if req.lodgings.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "lodgings list is empty".to_string()));
    }
    if req.total_route_km <= 0.0 {
        return Err((StatusCode::BAD_REQUEST, "total_route_km must be > 0".to_string()));
    }
    tracing::info!(
        "Plan stages: {} lodgings, {:.1}km total, target={:.0}km/day",
        req.lodgings.len(),
        req.total_route_km,
        req.target_km_per_day
    );
    let resp = backend::lodgings::plan_stages(&req);
    tracing::info!("Plan stages: {} stages, avg {:.1}km/day", resp.num_stages, resp.avg_km_per_day);
    Ok(Json(resp))
}

/// Handler for /api/lodgings-along-route — find OSM tourism accommodations
/// within a buffer distance of a route polyline. Returns lodgings annotated
/// with their position along the route (cumulative km from start) and their
/// perpendicular distance to the closest segment.
async fn lodgings_along_route_handler(
    Json(req): Json<backend::lodgings::LodgingsRequest>,
) -> Result<Json<backend::lodgings::LodgingsResponse>, (StatusCode, String)> {
    if req.coords.len() < 2 {
        return Err((StatusCode::BAD_REQUEST, "coords must contain at least 2 points".to_string()));
    }
    if req.buffer_m <= 0.0 || req.buffer_m > 20_000.0 {
        return Err((StatusCode::BAD_REQUEST, "buffer_m must be in (0, 20000]".to_string()));
    }
    tracing::info!(
        "Lodgings request: {} coords, buffer={}m",
        req.coords.len(),
        req.buffer_m
    );
    match backend::lodgings::find_lodgings_along_route(&req.coords, req.buffer_m).await {
        Ok(resp) => {
            tracing::info!(
                "Lodgings: {} found along {:.1}km route",
                resp.count,
                resp.total_route_km
            );
            Ok(Json(resp))
        }
        Err(e) => {
            tracing::warn!("Lodgings query failed: {e}");
            Err((StatusCode::BAD_GATEWAY, e))
        }
    }
}

/// Handler for /api/log - write frontend log to file
async fn frontend_log_handler(
    Json(req): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    if let Some(msg) = req["msg"].as_str() {
        use std::io::Write;
        let timestamp = chrono::Local::now().format("%H:%M:%S%.3f");
        let line = format!("[{}] {}\n", timestamp, msg);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("frontend_debug.log")
        {
            let _ = f.write_all(line.as_bytes());
        }
        tracing::info!("[FRONTEND] {}", msg);
    }
    Json(serde_json::json!({"ok": true}))
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
    match engine.find_path_with_surfaces(&req) {
        Some((path, point_surfaces)) => {
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
                    snapped_waypoints: None,
                estimated_time_minutes: estimated_time,
                difficulty,
                surface_breakdown: None,
                segments: None,
                point_surfaces: Some(point_surfaces),
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

/// Bounding box covering `points`, widened by a ~5km margin.
fn bbox_with_margin(points: &[Coordinate]) -> BoundingBox {
    let mut min_lat = f64::MAX;
    let mut max_lat = f64::MIN;
    let mut min_lon = f64::MAX;
    let mut max_lon = f64::MIN;

    for coord in points {
        min_lat = min_lat.min(coord.lat);
        max_lat = max_lat.max(coord.lat);
        min_lon = min_lon.min(coord.lon);
        max_lon = max_lon.max(coord.lon);
    }

    let margin_deg = 5.0 / 111.0; // ~5km in degrees
    BoundingBox {
        min_lat: (min_lat - margin_deg).max(-90.0),
        max_lat: (max_lat + margin_deg).min(90.0),
        min_lon: (min_lon - margin_deg).clamp(-180.0, 180.0),
        max_lon: (max_lon + margin_deg).clamp(-180.0, 180.0),
    }
}

/// Insert intermediate coordinates so that every consecutive pair fits within the
/// graph size limit on its own.
///
/// Two clicks far apart — Paris and Lyon, say — form a single pair whose bounding
/// box exceeds the limit, and no grouping of waypoints can fix that. Splitting the
/// pair along the straight line between them turns it into several routable hops.
/// The inserted anchors constrain the route to roughly follow that line.
///
/// Returns the densified coordinates alongside a flag per coordinate marking the
/// ones the caller actually asked for, so snapped positions and per-segment stats
/// stay aligned with the original waypoints.
fn densify_points(points: &[Coordinate]) -> Result<(Vec<Coordinate>, Vec<bool>), String> {
    // A pair needing more splits than this is beyond anything a hiking route needs;
    // bail out rather than spin.
    const MAX_SPLITS: usize = 64;

    let mut dense = Vec::with_capacity(points.len());
    let mut is_original = Vec::with_capacity(points.len());

    for (i, pair) in points.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);

        // Smallest split count whose sub-pairs all fit. Sub-pairs are evenly spaced,
        // so checking the first one covers them all.
        let mut splits = 1;
        while splits <= MAX_SPLITS {
            let step = Coordinate {
                lat: a.lat + (b.lat - a.lat) / splits as f64,
                lon: a.lon + (b.lon - a.lon) / splits as f64,
            };
            if bbox_with_margin(&[a, step]).validate().is_ok() {
                break;
            }
            splits += 1;
        }

        if splits > MAX_SPLITS {
            return Err(format!(
                "waypoints {} and {} are too far apart to route",
                i + 1,
                i + 2
            ));
        }

        dense.push(a);
        is_original.push(true);

        for k in 1..splits {
            let t = k as f64 / splits as f64;
            dense.push(Coordinate {
                lat: a.lat + (b.lat - a.lat) * t,
                lon: a.lon + (b.lon - a.lon) * t,
            });
            is_original.push(false);
        }
    }

    if let Some(&last) = points.last() {
        dense.push(last);
        is_original.push(true);
    }

    Ok((dense, is_original))
}

/// Split waypoints into contiguous chunks whose bounding box stays within the
/// graph size limit. Long imported traces (a GPX crossing several regions) would
/// otherwise be rejected outright: one graph covering the whole trace is both
/// refused by `BoundingBox::validate` and far too costly to build.
///
/// Each chunk restarts on the last point of the previous one, so consecutive
/// chunks share a waypoint and the routed path stays continuous.
///
/// Returns inclusive `(start, end)` index pairs into `points`, or an error when a
/// single pair of consecutive waypoints is already too far apart to ever fit —
/// no split can rescue that case.
fn chunk_points(points: &[Coordinate]) -> Result<Vec<(usize, usize)>, String> {
    let mut chunks = Vec::new();
    let mut start = 0;

    while start + 1 < points.len() {
        // Grow the chunk while the bbox of points[start..=end] stays valid.
        let mut end = start + 1;
        // densify_points guarantees every consecutive pair fits; this only fires on
        // degenerate input (identical or non-finite coordinates).
        if bbox_with_margin(&points[start..=end]).validate().is_err() {
            return Err(format!(
                "waypoints {} and {} cannot be routed (invalid bounding box)",
                start + 1,
                end + 1
            ));
        }

        while end + 1 < points.len()
            && bbox_with_margin(&points[start..=end + 1]).validate().is_ok()
        {
            end += 1;
        }

        chunks.push((start, end));
        start = end;
    }

    Ok(chunks)
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

    // Split pairs that are too far apart on their own, then group what remains into
    // chunks small enough to build a graph for. A short trace yields a single chunk
    // and the behaviour is unchanged; a long one is routed piecewise.
    let requested_count = points.len();
    let (points, is_original) = densify_points(&points)
        .map_err(|err_msg| (StatusCode::BAD_REQUEST, format!("Invalid request: {}", err_msg)))?;

    let chunks = chunk_points(&points)
        .map_err(|err_msg| (StatusCode::BAD_REQUEST, format!("Invalid request: {}", err_msg)))?;

    if points.len() > requested_count {
        tracing::info!(
            "Densified {} waypoints to {} (inserted anchors for long legs)",
            requested_count,
            points.len()
        );
    }

    tracing::info!(
        "Routing {} waypoints in {} chunk(s)",
        points.len(),
        chunks.len()
    );

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
    let mut all_surfaces: Vec<bool> = Vec::new();
    let mut snapped_waypoints: Vec<Coordinate> = Vec::new();
    let mut total_distance = 0.0;
    // Track segment boundaries: (start_idx, end_idx) in all_coords. Boundaries are
    // recorded at requested waypoints only, so inserted anchors stay invisible.
    let mut segment_boundaries: Vec<(usize, usize)> = Vec::new();
    let mut current_segment_start = 0usize;

    let t_pathfinding = std::time::Instant::now();
    for (chunk_idx, &(chunk_start, chunk_end)) in chunks.iter().enumerate() {
        // One graph per chunk. ENGINE_CACHE holds a single entry, so each chunk
        // evicts the previous one; chunks are routed in order and appended.
        let bbox = bbox_with_margin(&points[chunk_start..=chunk_end]);
        tracing::info!(
            "Chunk {}/{}: waypoints {}..={}",
            chunk_idx + 1,
            chunks.len(),
            chunk_start + 1,
            chunk_end + 1
        );
        let engine = get_or_build_engine(&config, bbox).await?;

        for i in chunk_start..chunk_end {
            let segment_req = RouteRequest {
                start: points[i],
                end: points[i + 1],
                w_pop: req.w_pop,
                w_paved: req.w_paved,
            };

            let t_seg = std::time::Instant::now();
            match engine.find_path_with_surfaces(&segment_req) {
                Some((path, seg_surfaces)) => {
                    tracing::info!(
                        "PERF segment {}/{}: {:.0}ms ({} pts)",
                        i + 1,
                        points.len() - 1,
                        t_seg.elapsed().as_secs_f64() * 1000.0,
                        path.len()
                    );

                    // Collect snapped positions: path starts at snap(points[i]),
                    // ends at snap(points[i+1]). Anchors inserted by densify_points
                    // are skipped — the frontend only knows about requested waypoints.
                    if i == 0 {
                        snapped_waypoints.push(path[0]);
                    }

                    // Add the routed path (dedup avoids duplicate at segment boundaries)
                    for (&coord, &surf) in path.iter().zip(seg_surfaces.iter()) {
                        let prev_len = all_coords.len();
                        push_dedup(&mut all_coords, coord);
                        if all_coords.len() > prev_len {
                            all_surfaces.push(surf);
                        }
                    }

                    let end_idx = all_coords.len() - 1;
                    if is_original[i + 1] {
                        if let Some(&last) = path.last() {
                            snapped_waypoints.push(last);
                        }
                        segment_boundaries.push((current_segment_start, end_idx));
                        current_segment_start = end_idx;
                    }

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
        snapped_waypoints: Some(snapped_waypoints),
        estimated_time_minutes: estimated_time,
        difficulty,
        surface_breakdown: None,
        segments,
        point_surfaces: Some(all_surfaces),
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
        .route("/api/log", axum::routing::post(frontend_log_handler))
        .route("/api/click_mode", axum::routing::get(click_mode_handler))
        .route("/api/pois", axum::routing::get(pois_handler))
        .route("/api/lodgings-along-route", axum::routing::post(lodgings_along_route_handler))
        .route("/api/plan-stages", axum::routing::post(plan_stages_handler))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn c(lat: f64, lon: f64) -> Coordinate {
        Coordinate { lat, lon }
    }

    /// Coordinate has no PartialEq; compare the components.
    fn same(a: Coordinate, b: Coordinate) -> bool {
        (a.lat - b.lat).abs() < 1e-9 && (a.lon - b.lon).abs() < 1e-9
    }

    /// A trace small enough for one graph must stay a single chunk, so short
    /// routes keep the exact behaviour they had before chunking.
    #[test]
    fn short_trace_is_one_chunk() {
        let points = vec![
            c(45.930, 4.577),
            c(45.933, 4.578),
            c(45.940, 4.577),
        ];
        assert_eq!(chunk_points(&points).unwrap(), vec![(0, 2)]);
    }

    /// A trace spanning several regions is split, and consecutive chunks share a
    /// waypoint so the concatenated path has no gap.
    #[test]
    fn long_trace_is_split_on_shared_waypoints() {
        // Charente -> Beaujolais, the span that used to be rejected outright.
        let points: Vec<Coordinate> = (0..15)
            .map(|i| c(45.93 + 0.05 * i as f64, 0.82 + 0.27 * i as f64))
            .collect();

        let chunks = chunk_points(&points).unwrap();
        assert!(chunks.len() > 1, "expected a split, got {:?}", chunks);
        assert_eq!(chunks.first().unwrap().0, 0);
        assert_eq!(chunks.last().unwrap().1, points.len() - 1);
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].1, pair[1].0, "chunks must share a waypoint");
        }
        for &(start, end) in &chunks {
            assert!(start < end, "a chunk must span at least one segment");
            assert!(bbox_with_margin(&points[start..=end]).validate().is_ok());
        }
    }

    /// A pair close enough to route on its own is left untouched.
    #[test]
    fn short_pair_is_not_densified() {
        let points = vec![c(45.930, 4.577), c(45.940, 4.578)];
        let (dense, is_original) = densify_points(&points).unwrap();
        assert_eq!(dense.len(), 2);
        assert!(same(dense[0], points[0]) && same(dense[1], points[1]));
        assert_eq!(is_original, vec![true, true]);
    }

    /// Two clicks far apart — the case that used to be rejected — gain anchors along
    /// the straight line between them, and every resulting hop is routable.
    #[test]
    fn long_pair_gains_anchors() {
        let points = vec![c(45.93, 0.82), c(46.64, 4.58)];
        let (dense, is_original) = densify_points(&points).unwrap();

        assert!(dense.len() > 2, "expected inserted anchors, got {:?}", dense);
        assert!(same(dense[0], points[0]));
        assert!(same(*dense.last().unwrap(), *points.last().unwrap()));
        assert_eq!(is_original.first(), Some(&true));
        assert_eq!(is_original.last(), Some(&true));
        assert_eq!(
            is_original.iter().filter(|&&o| o).count(),
            2,
            "only the two clicks are original"
        );
        for pair in dense.windows(2) {
            assert!(bbox_with_margin(pair).validate().is_ok());
        }
        // Densified input is always chunkable.
        assert!(chunk_points(&dense).is_ok());
    }

    /// Anchors are inserted only in the legs that need them, and the flags keep
    /// pointing at the requested waypoints.
    #[test]
    fn anchors_are_inserted_per_leg() {
        let points = vec![
            c(45.930, 4.577),
            c(45.940, 4.578),  // short leg
            c(46.640, 0.820),  // long leg
        ];
        let (dense, is_original) = densify_points(&points).unwrap();

        assert_eq!(dense.len(), is_original.len());
        let originals: Vec<Coordinate> = dense
            .iter()
            .zip(&is_original)
            .filter(|(_, &o)| o)
            .map(|(&c, _)| c)
            .collect();
        assert_eq!(originals.len(), points.len());
        assert!(originals
            .iter()
            .zip(&points)
            .all(|(&a, &b)| same(a, b)));
    }
}
