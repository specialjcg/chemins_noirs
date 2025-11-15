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
