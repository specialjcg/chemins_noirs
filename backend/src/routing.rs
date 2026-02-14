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

#[cfg(test)]
mod tests {
    use super::*;

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
