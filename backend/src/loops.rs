use std::{collections::HashSet, f64::consts::PI};

use petgraph::graph::NodeIndex;

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

/// Generate loop routes using geometric waypoint placement algorithm
///
/// # Algorithm: Multi-Ring Radial Sampling
///
/// This algorithm generates closed-loop routes by:
///
/// ## 1. Waypoint Generation Strategy
/// - Place intermediate waypoints on concentric circles (rings) around start
/// - Ring distances: [0.75×, 1.0×, 1.25×] of half_target_distance
/// - Points evenly distributed by bearing angle (2π / attempts_per_ring)
///
/// ## 2. Route Construction
/// For each candidate waypoint:
/// ```text
/// Loop = A* (start → waypoint) + A* (waypoint → start)
///
/// with constraint: return path excludes outbound edges
/// ```
///
/// ## 3. Candidate Filtering
/// Accept only if:
/// - Total distance within tolerance: |distance - target| ≤ tolerance_km
/// - Total ascent within bounds: min_ascent ≤ ascent ≤ max_ascent
/// - Path has ≥ 3 points (prevents degenerate loops)
///
/// ## 4. Optimization Parameters
/// - `TARGET_RING_FACTORS = [0.75, 1.0, 1.25]`: Explore 3 distance scales
/// - `MAX_LOOP_CANDIDATES = 12`: Limit results to prevent overload
/// - Early termination when `candidate_goal` candidates found
///
/// # Example
/// For a 20km loop:
/// - Half distance = 10km
/// - Ring 1: waypoints at ~7.5km  (0.75 × 10km)
/// - Ring 2: waypoints at ~10km   (1.0 × 10km)
/// - Ring 3: waypoints at ~12.5km (1.25 × 10km)
/// - Each ring: 8-12 bearing angles tested
///
/// # Returns
/// - `Ok(LoopRouteResponse)`: List of valid loop candidates sorted by quality
/// - `Err(LoopGenerationError)`: If no valid loops found or invalid parameters
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
    let candidate_goal = req.candidate_count.clamp(1, MAX_LOOP_CANDIDATES);
    let attempts_per_ring = candidate_goal.max(4);
    let half_distance = (req.target_distance_km / 2.0).max(0.5);

    tracing::info!(
        "Generating loops: target {:.1}km ± {:.1}km, {} candidates goal, {} attempts per ring ({} rings)",
        req.target_distance_km, tolerance, candidate_goal, attempts_per_ring, TARGET_RING_FACTORS.len()
    );

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
                tracing::debug!("Rejected: no path found to/from waypoint at bearing {:.0}°", bearing.to_degrees());
                continue;
            };
            if loop_path.len() < 3 {
                tracing::debug!("Rejected: path too short ({} points)", loop_path.len());
                continue;
            }

            let distance_km = approximate_distance_km(&loop_path);
            let distance_error = (distance_km - req.target_distance_km).abs();
            if distance_error > tolerance {
                tracing::debug!(
                    "Rejected: distance {:.1}km out of tolerance (target {:.1}km ± {:.1}km, error {:.1}km)",
                    distance_km, req.target_distance_km, tolerance, distance_error
                );
                continue;
            }

            let elevation_profile = create_elevation_profile(&loop_path)
                .await
                .map_err(|err| LoopGenerationError::Elevation(err.to_string()))?;
            if let Some(max_ascent) = req.max_total_ascent {
                if elevation_profile.total_ascent > max_ascent {
                    tracing::debug!(
                        "Rejected: ascent {:.0}m exceeds max {:.0}m",
                        elevation_profile.total_ascent, max_ascent
                    );
                    continue;
                }
            }
            if let Some(min_ascent) = req.min_total_ascent {
                if elevation_profile.total_ascent < min_ascent {
                    tracing::debug!(
                        "Rejected: ascent {:.0}m below min {:.0}m",
                        elevation_profile.total_ascent, min_ascent
                    );
                    continue;
                }
            }

            tracing::info!(
                "✓ Accepted loop #{}: {:.1}km, bearing {:.0}°, ascent {:.0}m",
                candidates.len() + 1,
                distance_km,
                normalize_bearing(bearing.to_degrees()),
                elevation_profile.total_ascent
            );

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

/// Build a complete loop path: start → waypoint → start
///
/// # Algorithm
/// 1. **Outbound**: A* from start to waypoint (unrestricted)
/// 2. **Track edges**: Record all edges used in outbound path
/// 3. **Return**: A* from waypoint to start, **excluding** outbound edges
///
/// This ensures the return path differs from outbound, creating a true loop.
///
/// # Edge Exclusion Strategy
/// - Excluded edges get 10× cost penalty (not fully blocked)
/// - Allows reuse as last resort if no alternative exists
/// - Final edge to start is always allowed (to close the loop)
///
/// # Returns
/// - `Some(Vec<Coordinate>)`: Complete loop if both paths found
/// - `None`: If either outbound or return path fails
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

    // First, find the outbound path
    let outbound = engine.find_path(&to_waypoint)?;
    if outbound.is_empty() {
        return None;
    }

    // Extract edges used in outbound path by mapping coordinates to node indices
    let excluded_edges = extract_edges_from_path(engine, &outbound);

    // Now find the return path while avoiding outbound edges
    let from_waypoint = RouteRequest {
        start: waypoint,
        end: req.start,
        w_pop: req.w_pop,
        w_paved: req.w_paved,
    };

    let mut inbound = engine.find_path_with_excluded_edges(&from_waypoint, &excluded_edges)?;
    if inbound.is_empty() {
        return None;
    }

    // Merge paths
    let mut result = outbound;
    inbound.remove(0); // drop duplicate waypoint before concatenation
    result.extend(inbound);
    Some(result)
}

/// Extract edge pairs (node indices) from a path of coordinates
fn extract_edges_from_path(
    engine: &RouteEngine,
    path: &[Coordinate],
) -> HashSet<(NodeIndex, NodeIndex)> {
    let mut edges = HashSet::new();

    for window in path.windows(2) {
        if let (Some(from_idx), Some(to_idx)) = (
            engine.closest_node(window[0]),
            engine.closest_node(window[1]),
        ) {
            edges.insert((from_idx, to_idx));
        }
    }

    edges
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_longitude() {
        assert_eq!(normalize_longitude(0.0), 0.0);
        assert_eq!(normalize_longitude(180.0), 180.0);
        assert_eq!(normalize_longitude(-180.0), -180.0);
        assert_eq!(normalize_longitude(190.0), -170.0);
        assert_eq!(normalize_longitude(-190.0), 170.0);
        assert_eq!(normalize_longitude(370.0), 10.0);
        assert_eq!(normalize_longitude(-370.0), -10.0);
    }

    #[test]
    fn test_normalize_bearing() {
        assert_eq!(normalize_bearing(0.0), 0.0);
        assert_eq!(normalize_bearing(90.0), 90.0);
        assert_eq!(normalize_bearing(180.0), 180.0);
        assert_eq!(normalize_bearing(270.0), 270.0);
        assert_eq!(normalize_bearing(360.0), 0.0);
        assert_eq!(normalize_bearing(-90.0), 270.0);
        assert_eq!(normalize_bearing(-180.0), 180.0);
        assert_eq!(normalize_bearing(450.0), 90.0);
    }

    #[test]
    fn test_destination_point_north() {
        // Starting point
        let start = Coordinate {
            lat: 45.0,
            lon: 5.0,
        };
        // Move 10km north (bearing = 0)
        let dest = destination_point(start, 10.0, 0.0);

        // At 45° latitude, 1° lat ≈ 111km
        // So 10km north ≈ 0.09° latitude increase
        assert!((dest.lat - 45.09).abs() < 0.01);
        assert!((dest.lon - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_destination_point_east() {
        let start = Coordinate {
            lat: 45.0,
            lon: 5.0,
        };
        // Move 10km east (bearing = 90°)
        let dest = destination_point(start, 10.0, std::f64::consts::PI / 2.0);

        // Should increase longitude, latitude stays roughly same
        assert!((dest.lat - 45.0).abs() < 0.01);
        assert!(dest.lon > 5.0);
        assert!(dest.lon < 5.2); // ~10km at 45° latitude
    }

    #[test]
    fn test_destination_point_south() {
        let start = Coordinate {
            lat: 45.0,
            lon: 5.0,
        };
        // Move 10km south (bearing = 180°)
        let dest = destination_point(start, 10.0, std::f64::consts::PI);

        // Should decrease latitude
        assert!(dest.lat < 45.0);
        assert!((dest.lat - 44.91).abs() < 0.01);
        assert!((dest.lon - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_destination_point_west() {
        let start = Coordinate {
            lat: 45.0,
            lon: 5.0,
        };
        // Move 10km west (bearing = 270°)
        let dest = destination_point(start, 10.0, 3.0 * std::f64::consts::PI / 2.0);

        // Should decrease longitude
        assert!((dest.lat - 45.0).abs() < 0.01);
        assert!(dest.lon < 5.0);
    }

    #[test]
    fn test_destination_point_zero_distance() {
        let start = Coordinate {
            lat: 45.0,
            lon: 5.0,
        };
        let dest = destination_point(start, 0.0, 0.0);

        // No movement (allow for floating point precision)
        assert!((dest.lat - start.lat).abs() < 1e-10);
        assert!((dest.lon - start.lon).abs() < 1e-10);
    }

    #[test]
    fn test_destination_point_crosses_antimeridian() {
        let start = Coordinate {
            lat: 0.0,
            lon: 179.0,
        };
        // Move east across antimeridian
        let dest = destination_point(start, 200.0, std::f64::consts::PI / 2.0);

        // Should wrap around to negative longitude
        assert!(dest.lon < -170.0);
        assert!(dest.lon > -180.0);
    }

    #[test]
    fn test_destination_point_near_pole() {
        let start = Coordinate {
            lat: 89.0,
            lon: 0.0,
        };
        // Move north near pole
        let dest = destination_point(start, 100.0, 0.0);

        // Should approach but not exceed 90°
        assert!(dest.lat > 89.0);
        assert!(dest.lat <= 90.0);
    }

    // Property-based tests using proptest
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        // Strategy for valid latitudes [-90, 90]
        fn valid_lat() -> impl Strategy<Value = f64> {
            -90.0..=90.0
        }

        // Strategy for valid longitudes [-180, 180]
        fn valid_lon() -> impl Strategy<Value = f64> {
            -180.0..=180.0
        }

        // Strategy for reasonable distances in km [0, 1000]
        fn valid_distance() -> impl Strategy<Value = f64> {
            0.0..=1000.0
        }

        // Strategy for bearings in radians [0, 2π]
        fn valid_bearing() -> impl Strategy<Value = f64> {
            0.0..=(2.0 * std::f64::consts::PI)
        }

        proptest! {
            #[test]
            fn prop_normalize_longitude_stays_in_range(lon in any::<f64>().prop_filter("finite", |x| x.is_finite())) {
                let normalized = normalize_longitude(lon);
                prop_assert!(normalized >= -180.0);
                prop_assert!(normalized <= 180.0);
            }

            #[test]
            fn prop_normalize_bearing_stays_in_range(bearing in any::<f64>().prop_filter("finite", |x| x.is_finite())) {
                let normalized = normalize_bearing(bearing);
                prop_assert!(normalized >= 0.0);
                prop_assert!(normalized < 360.0);
            }

            #[test]
            fn prop_destination_point_returns_valid_coords(
                lat in valid_lat(),
                lon in valid_lon(),
                distance in valid_distance(),
                bearing in valid_bearing()
            ) {
                let start = Coordinate { lat, lon };
                let dest = destination_point(start, distance, bearing);

                // Latitude should stay in valid range
                prop_assert!(dest.lat >= -90.0);
                prop_assert!(dest.lat <= 90.0);

                // Longitude should stay in valid range
                prop_assert!(dest.lon >= -180.0);
                prop_assert!(dest.lon <= 180.0);
            }

            #[test]
            fn prop_destination_point_zero_distance_returns_start(
                lat in valid_lat(),
                lon in valid_lon(),
                bearing in valid_bearing()
            ) {
                let start = Coordinate { lat, lon };
                let dest = destination_point(start, 0.0, bearing);

                // Should return approximately the same point
                prop_assert!((dest.lat - start.lat).abs() < 1e-9);
                prop_assert!((dest.lon - start.lon).abs() < 1e-9);
            }

            #[test]
            fn prop_normalize_longitude_idempotent(lon in valid_lon()) {
                let normalized_once = normalize_longitude(lon);
                let normalized_twice = normalize_longitude(normalized_once);
                prop_assert_eq!(normalized_once, normalized_twice);
            }

            #[test]
            fn prop_normalize_bearing_idempotent(bearing in 0.0..360.0) {
                let normalized_once = normalize_bearing(bearing);
                let normalized_twice = normalize_bearing(normalized_once);
                prop_assert_eq!(normalized_once, normalized_twice);
            }

            #[test]
            fn prop_normalize_bearing_addition_mod_360(
                bearing in 0.0..360.0,
                offset in 0.0..360.0
            ) {
                let sum: f64 = normalize_bearing(bearing + offset);
                let expected: f64 = (bearing + offset) % 360.0;
                prop_assert!((sum - expected).abs() < 1e-10);
            }
        }
    }
}
