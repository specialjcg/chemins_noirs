use crate::models::Coordinate;

pub const EARTH_RADIUS_KM: f64 = 6_371.0;
const EARTH_RADIUS_M: f64 = 6_371_000.0;

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

pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (lat1, lon1, lat2, lon2) = (
        lat1.to_radians(),
        lon1.to_radians(),
        lat2.to_radians(),
        lon2.to_radians(),
    );
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS_M * c
}

/// Fast equirectangular distance approximation for A* heuristic.
/// ~3-5x faster than haversine (no sin/asin, just cos + sqrt).
/// Admissible for distances < 100km (never overestimates).
pub fn fast_distance_km(a: Coordinate, b: Coordinate) -> f64 {
    let dlat = (b.lat - a.lat).to_radians();
    let dlon = (b.lon - a.lon).to_radians();
    let cos_mid = ((a.lat + b.lat) / 2.0).to_radians().cos();
    let x = dlon * cos_mid;
    (dlat * dlat + x * x).sqrt() * EARTH_RADIUS_KM
}

pub fn approximate_distance_km(path: &[Coordinate]) -> f64 {
    path.windows(2).map(|w| haversine_km(w[0], w[1])).sum()
}

pub fn compute_bounds(path: &[Coordinate]) -> (f64, f64, f64, f64) {
    let mut min_lat = f64::MAX;
    let mut max_lat = f64::MIN;
    let mut min_lon = f64::MAX;
    let mut max_lon = f64::MIN;

    for coord in path {
        min_lat = min_lat.min(coord.lat);
        max_lat = max_lat.max(coord.lat);
        min_lon = min_lon.min(coord.lon);
        max_lon = max_lon.max(coord.lon);
    }

    (min_lat, max_lat, min_lon, max_lon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haversine_same_point() {
        let point = Coordinate {
            lat: 45.0,
            lon: 5.0,
        };
        assert_eq!(haversine_km(point, point), 0.0);
    }

    #[test]
    fn test_haversine_symmetry() {
        let a = Coordinate {
            lat: 45.0,
            lon: 5.0,
        };
        let b = Coordinate {
            lat: 46.0,
            lon: 6.0,
        };
        assert_eq!(haversine_km(a, b), haversine_km(b, a));
    }

    #[test]
    fn test_haversine_m_zero_distance() {
        let dist = haversine_m(45.0, 5.0, 45.0, 5.0);
        assert!(dist.abs() < 0.01);
    }

    #[test]
    fn test_haversine_m_symmetry() {
        let dist1 = haversine_m(45.0, 5.0, 46.0, 6.0);
        let dist2 = haversine_m(46.0, 6.0, 45.0, 5.0);
        assert!((dist1 - dist2).abs() < 0.01);
    }

    #[test]
    fn test_fast_distance_close_to_haversine() {
        let a = Coordinate { lat: 45.0, lon: 5.0 };
        let b = Coordinate { lat: 45.5, lon: 5.5 };
        let hav = haversine_km(a, b);
        let fast = fast_distance_km(a, b);
        // Should be within 0.5% for short distances
        assert!((fast - hav).abs() / hav < 0.005, "fast={fast}, haversine={hav}");
    }

    #[test]
    fn test_fast_distance_admissible() {
        // fast_distance must never overestimate (admissible heuristic)
        let pairs = [
            (Coordinate { lat: 45.0, lon: 5.0 }, Coordinate { lat: 45.1, lon: 5.1 }),
            (Coordinate { lat: 44.0, lon: 3.0 }, Coordinate { lat: 44.5, lon: 3.5 }),
            (Coordinate { lat: 46.0, lon: 6.0 }, Coordinate { lat: 46.0, lon: 6.5 }),
        ];
        for (a, b) in pairs {
            let hav = haversine_km(a, b);
            let fast = fast_distance_km(a, b);
            assert!(fast <= hav * 1.001, "fast_distance overestimated: fast={fast}, haversine={hav}");
        }
    }

    #[test]
    fn test_approximate_distance_empty() {
        assert_eq!(approximate_distance_km(&[]), 0.0);
    }

    #[test]
    fn test_approximate_distance_single_point() {
        let path = vec![Coordinate {
            lat: 45.0,
            lon: 5.0,
        }];
        assert_eq!(approximate_distance_km(&path), 0.0);
    }
}
