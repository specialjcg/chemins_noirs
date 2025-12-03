use std::f64::consts::PI;

use crate::{
    elevation::create_elevation_profile,
    engine::RouteEngine,
    error::RouteError,
    gpx_export::encode_route_as_gpx,
    models::{
        Coordinate, LoopCandidate, LoopRouteRequest, LoopRouteResponse, RouteRequest, RouteResponse,
    },
    routing::approximate_distance_km,
};

const EARTH_RADIUS_KM: f64 = 6_371.0;
const MIN_TARGET_DISTANCE_KM: f64 = 2.0;
const MIN_DISTANCE_TOLERANCE_KM: f64 = 0.5;
const MAX_LOOP_CANDIDATES: usize = 12;
const TARGET_RING_FACTORS: [f64; 3] = [0.75, 1.0, 1.25];

#[derive(Debug, thiserror::Error)]
pub enum LoopGenerationError {
    #[error("loop distance must be strictly positive and larger than {MIN_TARGET_DISTANCE_KM} km")]
    InvalidTargetDistance,
    #[error("no loop could be generated with the provided constraints")]
    NoLoopFound,
    #[error(transparent)]
    Gpx(#[from] RouteError),
    #[error("failed to fetch elevation data: {0}")]
    Elevation(String),
}

pub async fn generate_loops(
    engine: &RouteEngine,
    req: &LoopRouteRequest,
) -> Result<LoopRouteResponse, LoopGenerationError> {
    if !req.target_distance_km.is_finite() || req.target_distance_km <= MIN_TARGET_DISTANCE_KM {
        return Err(LoopGenerationError::InvalidTargetDistance);
    }

    let tolerance = req
        .distance_tolerance_km
        .max(MIN_DISTANCE_TOLERANCE_KM)
        .min(req.target_distance_km);
    let candidate_goal = req.candidate_count.max(1).min(MAX_LOOP_CANDIDATES);
    let attempts_per_ring = candidate_goal.max(4);
    let half_distance = (req.target_distance_km / 2.0).max(0.5);

    let mut candidates = Vec::new();

    'rings: for (ring_idx, factor) in TARGET_RING_FACTORS.iter().enumerate() {
        for step in 0..attempts_per_ring {
            if candidates.len() >= candidate_goal {
                break 'rings;
            }

            let phase_offset = ring_idx as f64 * 0.35;
            let bearing = 2.0 * PI * (step as f64 / attempts_per_ring as f64) + phase_offset;
            let waypoint = destination_point(req.start, half_distance * factor, bearing);

            let Some(loop_path) = build_loop_path(engine, req, waypoint) else {
                continue;
            };
            if loop_path.len() < 3 {
                continue;
            }

            let distance_km = approximate_distance_km(&loop_path);
            let distance_error = (distance_km - req.target_distance_km).abs();
            if distance_error > tolerance {
                continue;
            }

            let elevation_profile = create_elevation_profile(&loop_path)
                .await
                .map_err(|err| LoopGenerationError::Elevation(err.to_string()))?;
            if let Some(max_ascent) = req.max_total_ascent {
                if elevation_profile.total_ascent > max_ascent {
                    continue;
                }
            }
            if let Some(min_ascent) = req.min_total_ascent {
                if elevation_profile.total_ascent < min_ascent {
                    continue;
                }
            }

            let gpx_base64 = encode_route_as_gpx(&loop_path)?;
            let metadata = Some(crate::build_metadata(&loop_path));
            let route = RouteResponse {
                path: loop_path,
                distance_km,
                gpx_base64,
                metadata,
                elevation_profile: Some(elevation_profile),
                terrain: None,
            };

            candidates.push(LoopCandidate {
                route,
                distance_error_km: distance_error,
                bearing_deg: normalize_bearing(bearing.to_degrees()),
            });
        }
    }

    if candidates.is_empty() {
        return Err(LoopGenerationError::NoLoopFound);
    }

    candidates.sort_by(|a, b| {
        let ascent_a = a
            .route
            .elevation_profile
            .as_ref()
            .map(|profile| profile.total_ascent)
            .unwrap_or(f64::MAX);
        let ascent_b = b
            .route
            .elevation_profile
            .as_ref()
            .map(|profile| profile.total_ascent)
            .unwrap_or(f64::MAX);

        ascent_a
            .partial_cmp(&ascent_b)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.distance_error_km
                    .partial_cmp(&b.distance_error_km)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    candidates.truncate(candidate_goal);

    Ok(LoopRouteResponse {
        target_distance_km: req.target_distance_km,
        distance_tolerance_km: tolerance,
        candidates,
    })
}

fn build_loop_path(
    engine: &RouteEngine,
    req: &LoopRouteRequest,
    waypoint: Coordinate,
) -> Option<Vec<Coordinate>> {
    let to_waypoint = RouteRequest {
        start: req.start,
        end: waypoint,
        w_pop: req.w_pop,
        w_paved: req.w_paved,
    };
    let from_waypoint = RouteRequest {
        start: waypoint,
        end: req.start,
        w_pop: req.w_pop,
        w_paved: req.w_paved,
    };

    let mut outbound = engine.find_path(&to_waypoint)?;
    let mut inbound = engine.find_path(&from_waypoint)?;
    if outbound.is_empty() || inbound.is_empty() {
        return None;
    }
    inbound.remove(0); // drop duplicate waypoint before concatenation
    outbound.extend(inbound);
    Some(outbound)
}

fn destination_point(start: Coordinate, distance_km: f64, bearing_rad: f64) -> Coordinate {
    let angular_distance = distance_km / EARTH_RADIUS_KM;
    let lat1 = start.lat.to_radians();
    let lon1 = start.lon.to_radians();

    let lat2 = f64::asin(
        lat1.sin() * angular_distance.cos()
            + lat1.cos() * angular_distance.sin() * bearing_rad.cos(),
    );
    let lon2 = lon1
        + f64::atan2(
            bearing_rad.sin() * angular_distance.sin() * lat1.cos(),
            angular_distance.cos() - lat1.sin() * lat2.sin(),
        );

    Coordinate {
        lat: lat2.to_degrees(),
        lon: normalize_longitude(lon2.to_degrees()),
    }
}

fn normalize_longitude(lon: f64) -> f64 {
    let mut normalized = lon;
    while normalized < -180.0 {
        normalized += 360.0;
    }
    while normalized > 180.0 {
        normalized -= 360.0;
    }
    normalized
}

fn normalize_bearing(bearing_deg: f64) -> f64 {
    let mut value = bearing_deg % 360.0;
    if value < 0.0 {
        value += 360.0;
    }
    value
}
