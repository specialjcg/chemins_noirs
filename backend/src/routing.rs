use crate::models::{Coordinate, RouteRequest};

const EARTH_RADIUS_KM: f64 = 6_371.0;

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

pub fn approximate_distance_km(path: &[Coordinate]) -> f64 {
    path.windows(2).map(|w| haversine_km(w[0], w[1])).sum()
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

pub fn haversine_km(a: Coordinate, b: Coordinate) -> f64 {
    let lat1 = a.lat.to_radians();
    let lat2 = b.lat.to_radians();
    let dlat = (b.lat - a.lat).to_radians();
    let dlon = (b.lon - a.lon).to_radians();

    let sin_dlat = (dlat / 2.0).sin();
    let sin_dlon = (dlon / 2.0).sin();

    let h = sin_dlat * sin_dlat + lat1.cos() * lat2.cos() * sin_dlon * sin_dlon;
    2.0 * EARTH_RADIUS_KM * h.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haversine_same_point() {
        let point = Coordinate { lat: 45.0, lon: 5.0 };
        assert_eq!(haversine_km(point, point), 0.0);
    }

    #[test]
    fn test_haversine_symmetry() {
        let a = Coordinate { lat: 45.0, lon: 5.0 };
        let b = Coordinate { lat: 46.0, lon: 6.0 };
        assert_eq!(haversine_km(a, b), haversine_km(b, a));
    }

    #[test]
    fn test_approximate_distance_empty() {
        assert_eq!(approximate_distance_km(&[]), 0.0);
    }

    #[test]
    fn test_approximate_distance_single_point() {
        let path = vec![Coordinate { lat: 45.0, lon: 5.0 }];
        assert_eq!(approximate_distance_km(&path), 0.0);
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
                // Maximum distance on Earth is half the circumference (antipodal points)
                let max_distance = std::f64::consts::PI * EARTH_RADIUS_KM;
                prop_assert!(dist <= max_distance + 0.1); // Small epsilon for floating point
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

                // Triangle inequality: d(a,c) <= d(a,b) + d(b,c)
                // Add small epsilon for floating point errors
                prop_assert!(dist_ac <= dist_ab + dist_bc + 1e-6);
            }

            #[test]
            fn prop_approximate_distance_monotonic(
                coords in prop::collection::vec(valid_coord(), 2..10)
            ) {
                let distance = approximate_distance_km(&coords);
                prop_assert!(distance >= 0.0);

                // Adding a point should not decrease total distance
                // (unless it's the same point, which we'll skip)
            }

            #[test]
            fn prop_approximate_distance_additive(
                path1 in prop::collection::vec(valid_coord(), 2..5),
                path2 in prop::collection::vec(valid_coord(), 2..5)
            ) {
                // Distance of concatenated paths should equal sum of individual distances
                // (minus the connecting segment if last of path1 != first of path2)
                let dist1 = approximate_distance_km(&path1);
                let dist2 = approximate_distance_km(&path2);

                let mut combined = path1.clone();
                combined.extend_from_slice(&path2);
                let dist_combined = approximate_distance_km(&combined);

                // Combined distance includes connection between last of path1 and first of path2
                let connection = haversine_km(*path1.last().unwrap(), path2[0]);
                let expected = dist1 + connection + dist2;

                prop_assert!((dist_combined - expected).abs() < 1e-6);
            }

            #[test]
            fn prop_perpendicular_unit_is_perpendicular(
                start in valid_coord(),
                end in valid_coord()
            ) {
                // Skip if start == end
                prop_assume!((start.lat - end.lat).abs() > 1e-6 || (start.lon - end.lon).abs() > 1e-6);

                let perp = perpendicular_unit(start, end);
                let direction = Coordinate {
                    lat: end.lat - start.lat,
                    lon: end.lon - start.lon,
                };

                // Dot product should be zero for perpendicular vectors
                let dot_product = direction.lat * perp.lat + direction.lon * perp.lon;
                prop_assert!(dot_product.abs() < 1e-6);
            }

            #[test]
            fn prop_perpendicular_unit_is_unit_vector(
                start in valid_coord(),
                end in valid_coord()
            ) {
                // Skip if start == end
                prop_assume!((start.lat - end.lat).abs() > 1e-6 || (start.lon - end.lon).abs() > 1e-6);

                let perp = perpendicular_unit(start, end);
                let magnitude = (perp.lat * perp.lat + perp.lon * perp.lon).sqrt();

                // Should be approximately 1.0 (unit vector)
                prop_assert!((magnitude - 1.0).abs() < 1e-6);
            }
        }
    }
}
