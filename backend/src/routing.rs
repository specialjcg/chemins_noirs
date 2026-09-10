use crate::models::{Coordinate, RouteRequest};

// Re-export from geo_utils for backward compatibility with external callers
pub use crate::geo_utils::{approximate_distance_km, haversine_km, EARTH_RADIUS_KM};

pub fn generate_route(req: &RouteRequest) -> Vec<Coordinate> {
    const STEPS: usize = 32;
    let mut path = Vec::with_capacity(STEPS + 1);
    let start = req.start;
    let end = req.end;
    let avoidance = (req.w_pop + req.w_paved).clamp(0.0, 10.0);
    let perp = perpendicular_unit(start, end);

    for i in 0..=STEPS {
        let t = i as f64 / STEPS as f64;
        let mut point = start.interpolate(end, t);
        let wiggle = ((i as f64) * 0.45).sin() * 0.01 * avoidance;
        point.lat += perp.lat * wiggle;
        point.lon += perp.lon * wiggle;
        path.push(point);
    }

    path
}

fn perpendicular_unit(start: Coordinate, end: Coordinate) -> Coordinate {
    let dx = end.lon - start.lon;
    let dy = end.lat - start.lat;
    let len = (dx * dx + dy * dy).sqrt().max(f64::EPSILON);
    Coordinate {
        lon: -dy / len,
        lat: dx / len,
    }
}

/// Estimate hiking time using Naismith's rule:
/// time_h = distance_km / 5.0 + ascent_m / 600.0
pub fn estimate_time_minutes(distance_km: f64, total_ascent: f64) -> u32 {
    let hours = distance_km / 5.0 + total_ascent / 600.0;
    (hours * 60.0).round() as u32
}

/// Shortest run of trail a slope is measured over, in metres.
///
/// Elevation is accurate to a metre or two at best, so over a few metres of
/// ground that noise *is* the slope: a 5 m step with 2.6 m between its ends
/// reads as 48 %, and one such point was enough to rate 152 km of flat
/// Sologne "expert". A real ramp survives being measured over 25 m; a bad
/// sample does not.
const MIN_SLOPE_RUN_M: f64 = 25.0;

/// Steepest slope over any stretch of at least `MIN_SLOPE_RUN_M`.
///
/// The window slides point by point and grows until it is long enough, so a
/// short sharp climb inside a longer stretch is still caught — it is only
/// measured over ground long enough to mean something.
fn steepest_run_pct(elevations: &[Option<f64>], path: &[Coordinate]) -> f64 {
    let count = path.len().min(elevations.len());
    let mut steepest: f64 = 0.0;

    for start in 0..count {
        let Some(from) = elevations[start] else { continue };

        let mut run_m = 0.0;
        for i in (start + 1)..count {
            run_m += haversine_km(path[i - 1], path[i]) * 1000.0;
            if run_m < MIN_SLOPE_RUN_M {
                continue;
            }

            if let Some(to) = elevations[i] {
                let slope = (to - from).abs() / run_m * 100.0;
                steepest = steepest.max(slope);
            }
            break;
        }
    }

    steepest
}

/// Rate difficulty based on max slope, total elevation, and distance.
/// Returns "easy", "moderate", "difficult", or "expert".
pub fn rate_difficulty(
    elevations: &[Option<f64>],
    path: &[Coordinate],
    total_ascent: f64,
) -> String {
    let max_slope_pct = steepest_run_pct(elevations, path);

    if max_slope_pct < 15.0 && total_ascent < 300.0 {
        "easy".to_string()
    } else if max_slope_pct < 25.0 && total_ascent < 600.0 {
        "moderate".to_string()
    } else if max_slope_pct < 35.0 && total_ascent < 1000.0 {
        "difficult".to_string()
    } else {
        "expert".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Points espacés de `spacing_m` le long d'un parallèle, aux altitudes données.
    fn profile(spacing_m: f64, elevations: &[f64]) -> (Vec<Option<f64>>, Vec<Coordinate>) {
        let step_deg = spacing_m / (111_320.0 * (45.0f64).to_radians().cos());
        let path = (0..elevations.len())
            .map(|i| Coordinate { lat: 45.0, lon: 5.0 + i as f64 * step_deg })
            .collect();
        (elevations.iter().map(|&e| Some(e)).collect(), path)
    }

    #[test]
    fn a_noisy_step_no_longer_decides_the_rating() {
        // 2,6 m d'écart sur 5 m de terrain : 48 % de pente sur le papier, du
        // bruit d'altimétrie en réalité. C'est le point qui classait 152 km de
        // Sologne plate en « expert ».
        let (elevations, path) = profile(5.0, &[74.6, 72.0, 74.6, 72.0, 74.6, 72.0]);
        assert!(
            steepest_run_pct(&elevations, &path) < 15.0,
            "pente retenue : {}",
            steepest_run_pct(&elevations, &path)
        );
        assert_eq!(rate_difficulty(&elevations, &path, 50.0), "easy");
    }

    #[test]
    fn a_real_climb_is_still_seen() {
        // 30 % soutenus sur 250 m : une vraie rampe, elle doit rester détectée.
        let mut elevations = vec![0.0];
        for i in 1..=10 {
            elevations.push(i as f64 * 7.5);
        }
        let (elevations, path) = profile(25.0, &elevations);

        let slope = steepest_run_pct(&elevations, &path);
        assert!((slope - 30.0).abs() < 1.0, "pente retenue : {slope}");
    }

    #[test]
    fn a_short_ramp_inside_a_long_stretch_is_caught() {
        // Rampe brève au milieu du plat : la fenêtre glissante doit la voir,
        // mesurée sur la longueur minimale et non diluée sur tout le tracé.
        let mut elevations = vec![100.0; 20];
        for i in 10..14 {
            elevations[i] = 100.0 + (i - 9) as f64 * 4.0;
        }
        for i in 14..20 {
            elevations[i] = 116.0;
        }
        let (elevations, path) = profile(10.0, &elevations);

        let slope = steepest_run_pct(&elevations, &path);
        assert!(slope > 30.0, "rampe manquée : {slope}");
    }

    #[test]
    fn a_flat_walk_stays_easy() {
        let (elevations, path) = profile(50.0, &[100.0; 40]);
        assert_eq!(rate_difficulty(&elevations, &path, 120.0), "easy");
        assert_eq!(steepest_run_pct(&elevations, &path), 0.0);
    }

    #[test]
    fn total_ascent_alone_still_raises_the_rating() {
        // La pente n'est pas le seul critère : le cumul compte aussi.
        let (elevations, path) = profile(50.0, &[100.0; 40]);
        assert_eq!(rate_difficulty(&elevations, &path, 800.0), "difficult");
        assert_eq!(rate_difficulty(&elevations, &path, 1200.0), "expert");
    }

    // Property-based tests using proptest
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn valid_coord() -> impl Strategy<Value = Coordinate> {
            (-90.0..=90.0, -180.0..=180.0)
                .prop_map(|(lat, lon)| Coordinate { lat, lon })
        }

        proptest! {
            #[test]
            fn prop_haversine_non_negative(a in valid_coord(), b in valid_coord()) {
                let dist = haversine_km(a, b);
                prop_assert!(dist >= 0.0);
            }

            #[test]
            fn prop_haversine_symmetric(a in valid_coord(), b in valid_coord()) {
                let dist_ab = haversine_km(a, b);
                let dist_ba = haversine_km(b, a);
                prop_assert!((dist_ab - dist_ba).abs() < 1e-10);
            }

            #[test]
            fn prop_haversine_same_point_is_zero(coord in valid_coord()) {
                let dist = haversine_km(coord, coord);
                prop_assert_eq!(dist, 0.0);
            }

            #[test]
            fn prop_haversine_bounded_by_half_earth_circumference(
                a in valid_coord(),
                b in valid_coord()
            ) {
                let dist = haversine_km(a, b);
                let max_distance = std::f64::consts::PI * EARTH_RADIUS_KM;
                prop_assert!(dist <= max_distance + 0.1);
            }

            #[test]
            fn prop_haversine_triangle_inequality(
                a in valid_coord(),
                b in valid_coord(),
                c in valid_coord()
            ) {
                let dist_ab = haversine_km(a, b);
                let dist_bc = haversine_km(b, c);
                let dist_ac = haversine_km(a, c);
                prop_assert!(dist_ac <= dist_ab + dist_bc + 1e-6);
            }

            #[test]
            fn prop_approximate_distance_monotonic(
                coords in prop::collection::vec(valid_coord(), 2..10)
            ) {
                let distance = approximate_distance_km(&coords);
                prop_assert!(distance >= 0.0);
            }

            #[test]
            fn prop_approximate_distance_additive(
                path1 in prop::collection::vec(valid_coord(), 2..5),
                path2 in prop::collection::vec(valid_coord(), 2..5)
            ) {
                let dist1 = approximate_distance_km(&path1);
                let dist2 = approximate_distance_km(&path2);

                let mut combined = path1.clone();
                combined.extend_from_slice(&path2);
                let dist_combined = approximate_distance_km(&combined);

                let connection = haversine_km(*path1.last().unwrap(), path2[0]);
                let expected = dist1 + connection + dist2;

                prop_assert!((dist_combined - expected).abs() < 1e-6);
            }

            #[test]
            fn prop_perpendicular_unit_is_perpendicular(
                start in valid_coord(),
                end in valid_coord()
            ) {
                prop_assume!((start.lat - end.lat).abs() > 1e-6 || (start.lon - end.lon).abs() > 1e-6);

                let perp = perpendicular_unit(start, end);
                let direction = Coordinate {
                    lat: end.lat - start.lat,
                    lon: end.lon - start.lon,
                };

                let dot_product = direction.lat * perp.lat + direction.lon * perp.lon;
                prop_assert!(dot_product.abs() < 1e-6);
            }

            #[test]
            fn prop_perpendicular_unit_is_unit_vector(
                start in valid_coord(),
                end in valid_coord()
            ) {
                prop_assume!((start.lat - end.lat).abs() > 1e-6 || (start.lon - end.lon).abs() > 1e-6);

                let perp = perpendicular_unit(start, end);
                let magnitude = (perp.lat * perp.lat + perp.lon * perp.lon).sqrt();

                prop_assert!((magnitude - 1.0).abs() < 1e-6);
            }
        }
    }
}
