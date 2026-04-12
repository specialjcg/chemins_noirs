//! Lodging discovery along a hiking route.
//!
//! Given a route polyline, queries OpenStreetMap (Overpass API) for tourism
//! accommodations within a buffer distance of the route, and returns them
//! annotated with their position along the route (cumulative km from start)
//! and their perpendicular distance to the closest segment.
//!
//! This is the foundation for the "stage planner" feature: client can take
//! the returned list, score each lodging, and decide where to break the tour
//! into N stages with the best gîtes at each cut.

use serde::{Deserialize, Serialize};

use crate::geo_utils::haversine_m;
use crate::models::Coordinate;

/// Tourism tags we consider as valid lodgings for hikers.
const TOURISM_KINDS: &[&str] = &[
    "guest_house",
    "hostel",
    "alpine_hut",
    "wilderness_hut",
    "chalet",
    "hotel",
    "apartment",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lodging {
    pub osm_id: i64,
    pub osm_type: String, // "node" | "way" | "relation"
    pub name: String,
    pub kind: String,
    pub lat: f64,
    pub lon: f64,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub email: Option<String>,
    pub capacity: Option<String>,
    pub stars: Option<String>,
    pub address: Option<String>,

    /// Perpendicular distance from the lodging to the nearest point on the route polyline.
    pub perpendicular_dist_m: f64,

    /// Cumulative distance from the start of the route to the foot of the perpendicular.
    /// Use this to decide which stage a lodging belongs to.
    pub along_route_km: f64,
}

#[derive(Debug, Deserialize)]
pub struct LodgingsRequest {
    pub coords: Vec<Coordinate>,
    /// Maximum perpendicular distance to the route polyline, in meters.
    /// Lodgings further than this are excluded from the result.
    pub buffer_m: f64,
}

#[derive(Debug, Serialize)]
pub struct LodgingsResponse {
    pub lodgings: Vec<Lodging>,
    pub count: usize,
    pub total_route_km: f64,
}

/// Compute the bbox (min_lat, max_lat, min_lon, max_lon) of a polyline,
/// expanded by `buffer_m` in every direction.
fn expanded_bbox(coords: &[Coordinate], buffer_m: f64) -> (f64, f64, f64, f64) {
    let mut min_lat = f64::MAX;
    let mut max_lat = f64::MIN;
    let mut min_lon = f64::MAX;
    let mut max_lon = f64::MIN;
    for c in coords {
        min_lat = min_lat.min(c.lat);
        max_lat = max_lat.max(c.lat);
        min_lon = min_lon.min(c.lon);
        max_lon = max_lon.max(c.lon);
    }
    // Convert buffer in meters to lat/lon degrees (approximate but safe over-estimate).
    let lat_buf = buffer_m / 111_000.0;
    let avg_lat_rad = ((min_lat + max_lat) / 2.0).to_radians();
    let lon_buf = buffer_m / (111_000.0 * avg_lat_rad.cos().abs().max(0.1));
    (
        min_lat - lat_buf,
        max_lat + lat_buf,
        min_lon - lon_buf,
        max_lon + lon_buf,
    )
}

/// Project a point P onto the segment AB and return (foot_lat, foot_lon, t)
/// where t is the parametric position in [0, 1] along the segment.
/// We use a local equirectangular projection (good enough at <10km segments).
fn project_on_segment(
    plat: f64,
    plon: f64,
    a_lat: f64,
    a_lon: f64,
    b_lat: f64,
    b_lon: f64,
) -> (f64, f64, f64) {
    let avg_lat_rad = ((a_lat + b_lat) / 2.0).to_radians();
    let cos = avg_lat_rad.cos();

    // Local meters from A
    let ax = 0.0;
    let ay = 0.0;
    let bx = (b_lon - a_lon) * 111_000.0 * cos;
    let by = (b_lat - a_lat) * 111_000.0;
    let px = (plon - a_lon) * 111_000.0 * cos;
    let py = (plat - a_lat) * 111_000.0;

    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-9 {
        return (a_lat, a_lon, 0.0);
    }
    let t = ((px - ax) * dx + (py - ay) * dy) / len_sq;
    let t_clamped = t.clamp(0.0, 1.0);

    let foot_x = ax + t_clamped * dx;
    let foot_y = ay + t_clamped * dy;
    let foot_lat = a_lat + foot_y / 111_000.0;
    let foot_lon = a_lon + foot_x / (111_000.0 * cos);
    (foot_lat, foot_lon, t_clamped)
}

/// For a point (lat, lon), compute (perpendicular_dist_m, along_route_km).
/// Iterates all segments of the polyline, picks the closest, and returns
/// the projection's distance along the cumulative polyline.
fn project_on_polyline(coords: &[Coordinate], lat: f64, lon: f64) -> (f64, f64) {
    if coords.len() < 2 {
        if let Some(c) = coords.first() {
            let d = haversine_m(c.lat, c.lon, lat, lon);
            return (d, 0.0);
        }
        return (f64::MAX, 0.0);
    }

    // Precompute cumulative distances (km) along the polyline up to each vertex
    let mut cum_km = Vec::with_capacity(coords.len());
    cum_km.push(0.0_f64);
    for i in 1..coords.len() {
        let prev_km = cum_km[i - 1];
        let seg_km = haversine_m(coords[i - 1].lat, coords[i - 1].lon, coords[i].lat, coords[i].lon)
            / 1000.0;
        cum_km.push(prev_km + seg_km);
    }

    let mut best_dist = f64::MAX;
    let mut best_along_km = 0.0_f64;

    for i in 0..coords.len() - 1 {
        let a = coords[i];
        let b = coords[i + 1];
        let (foot_lat, foot_lon, t) = project_on_segment(lat, lon, a.lat, a.lon, b.lat, b.lon);
        let d = haversine_m(lat, lon, foot_lat, foot_lon);
        if d < best_dist {
            best_dist = d;
            let seg_km = cum_km[i + 1] - cum_km[i];
            best_along_km = cum_km[i] + t * seg_km;
        }
    }

    (best_dist, best_along_km)
}

/// Build the Overpass QL query for the bbox + tourism kinds.
fn build_overpass_query(min_lat: f64, max_lat: f64, min_lon: f64, max_lon: f64) -> String {
    let kinds = TOURISM_KINDS.join("|");
    // bbox order in Overpass is south, west, north, east
    format!(
        r#"[out:json][timeout:60];
(
  nwr["tourism"~"^({kinds})$"]({min_lat},{min_lon},{max_lat},{max_lon});
);
out center tags;"#
    )
}

/// Query Overpass for lodgings in the given bbox. Tries the main endpoint
/// first then a mirror, with retries on transient failures.
async fn fetch_overpass(
    min_lat: f64,
    max_lat: f64,
    min_lon: f64,
    max_lon: f64,
) -> Result<serde_json::Value, String> {
    let query = build_overpass_query(min_lat, max_lat, min_lon, max_lon);

    // Try main endpoint then Kumi mirror — Overpass main is often saturated.
    let endpoints = [
        "https://overpass-api.de/api/interpreter",
        "https://overpass.kumi.systems/api/interpreter",
    ];

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(75))
        .build()
        .map_err(|e| format!("client build: {e}"))?;

    let mut last_err = String::new();
    for endpoint in endpoints {
        for attempt in 1..=2 {
            match client
                .post(endpoint)
                .form(&[("data", query.as_str())])
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        match resp.json::<serde_json::Value>().await {
                            Ok(json) => {
                                if json.get("elements").is_some() {
                                    return Ok(json);
                                }
                                last_err = format!("response missing 'elements' field");
                            }
                            Err(e) => {
                                last_err = format!("{endpoint}: parse error: {e}");
                            }
                        }
                    } else {
                        let body = resp.text().await.unwrap_or_default();
                        last_err = format!(
                            "{endpoint} HTTP {status}: {}",
                            body.chars().take(200).collect::<String>()
                        );
                    }
                }
                Err(e) => {
                    last_err = format!("{endpoint}: network error (attempt {attempt}): {e}");
                }
            }
            if attempt == 1 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
    Err(format!("Overpass failed on all endpoints: {last_err}"))
}

/// Parse one OSM element from the Overpass response into a Lodging struct,
/// or return None if it's missing required fields.
fn parse_element(el: &serde_json::Value) -> Option<Lodging> {
    let osm_id = el.get("id")?.as_i64()?;
    let osm_type = el.get("type")?.as_str()?.to_string();

    // Coordinates: nodes have lat/lon directly, ways/relations have a 'center'
    let (lat, lon) = if let (Some(lat), Some(lon)) = (
        el.get("lat").and_then(|v| v.as_f64()),
        el.get("lon").and_then(|v| v.as_f64()),
    ) {
        (lat, lon)
    } else if let Some(center) = el.get("center") {
        let lat = center.get("lat")?.as_f64()?;
        let lon = center.get("lon")?.as_f64()?;
        (lat, lon)
    } else {
        return None;
    };

    let tags = el.get("tags")?.as_object()?;
    let kind = tags.get("tourism")?.as_str()?.to_string();

    let s = |k: &str| tags.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    let any_of = |keys: &[&str]| -> Option<String> {
        for k in keys {
            if let Some(v) = tags.get(*k).and_then(|v| v.as_str()) {
                return Some(v.to_string());
            }
        }
        None
    };

    let name = s("name").unwrap_or_else(|| "(sans nom)".to_string());
    let phone = any_of(&["phone", "contact:phone"]);
    let website = any_of(&["website", "contact:website", "url"]);
    let email = any_of(&["email", "contact:email"]);
    let capacity = any_of(&["capacity", "beds", "rooms"]);
    let stars = s("stars");
    let address = {
        let street = s("addr:street");
        let housenumber = s("addr:housenumber");
        let city = s("addr:city");
        let postcode = s("addr:postcode");
        let parts: Vec<String> = [housenumber, street, postcode, city]
            .into_iter()
            .flatten()
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    };

    Some(Lodging {
        osm_id,
        osm_type,
        name,
        kind,
        lat,
        lon,
        phone,
        website,
        email,
        capacity,
        stars,
        address,
        perpendicular_dist_m: 0.0,
        along_route_km: 0.0,
    })
}

/// Main entry point: given a route polyline, return all lodgings within
/// `buffer_m` of the route, sorted by along-route distance.
pub async fn find_lodgings_along_route(
    coords: &[Coordinate],
    buffer_m: f64,
) -> Result<LodgingsResponse, String> {
    if coords.len() < 2 {
        return Err("coords must contain at least 2 points".to_string());
    }

    // 1. Compute expanded bbox
    let (min_lat, max_lat, min_lon, max_lon) = expanded_bbox(coords, buffer_m);

    // 2. Query Overpass
    let raw = fetch_overpass(min_lat, max_lat, min_lon, max_lon).await?;
    let elements = raw
        .get("elements")
        .and_then(|v| v.as_array())
        .ok_or("Overpass response missing 'elements'")?;

    // 3. Parse + project + filter
    let mut lodgings: Vec<Lodging> = elements
        .iter()
        .filter_map(parse_element)
        .filter_map(|mut l| {
            let (perp, along) = project_on_polyline(coords, l.lat, l.lon);
            if perp <= buffer_m {
                l.perpendicular_dist_m = perp;
                l.along_route_km = along;
                Some(l)
            } else {
                None
            }
        })
        .collect();

    // 4. Sort by along-route distance
    lodgings.sort_by(|a, b| {
        a.along_route_km
            .partial_cmp(&b.along_route_km)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 5. Compute total route length
    let total_route_km: f64 = coords
        .windows(2)
        .map(|w| haversine_m(w[0].lat, w[0].lon, w[1].lat, w[1].lon) / 1000.0)
        .sum();

    let count = lodgings.len();
    Ok(LodgingsResponse {
        lodgings,
        count,
        total_route_km,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coord(lat: f64, lon: f64) -> Coordinate {
        Coordinate { lat, lon }
    }

    #[test]
    fn expanded_bbox_grows_in_all_directions() {
        let coords = vec![coord(45.0, 5.0), coord(45.1, 5.1)];
        let (min_lat, max_lat, min_lon, max_lon) = expanded_bbox(&coords, 1000.0);
        assert!(min_lat < 45.0);
        assert!(max_lat > 45.1);
        assert!(min_lon < 5.0);
        assert!(max_lon > 5.1);
        // ~1km in lat ≈ 0.009 deg
        assert!((min_lat - (45.0 - 0.009)).abs() < 0.001);
    }

    #[test]
    fn project_on_segment_endpoint_a() {
        // Point exactly at A → t=0, foot=A
        let (lat, lon, t) = project_on_segment(45.0, 5.0, 45.0, 5.0, 45.1, 5.1);
        assert!((lat - 45.0).abs() < 1e-6);
        assert!((lon - 5.0).abs() < 1e-6);
        assert!(t.abs() < 1e-6);
    }

    #[test]
    fn project_on_segment_endpoint_b() {
        // Point exactly at B → t=1, foot=B
        let (lat, lon, t) = project_on_segment(45.1, 5.1, 45.0, 5.0, 45.1, 5.1);
        assert!((lat - 45.1).abs() < 1e-6);
        assert!((lon - 5.1).abs() < 1e-6);
        assert!((t - 1.0).abs() < 1e-6);
    }

    #[test]
    fn project_on_segment_midpoint() {
        // Point at midpoint → t=0.5
        let mid_lat = 45.05;
        let mid_lon = 5.05;
        let (lat, lon, t) = project_on_segment(mid_lat, mid_lon, 45.0, 5.0, 45.1, 5.1);
        assert!((lat - mid_lat).abs() < 1e-4);
        assert!((lon - mid_lon).abs() < 1e-4);
        assert!((t - 0.5).abs() < 0.01);
    }

    #[test]
    fn project_on_segment_clamped_before_a() {
        // Point before A on the line direction → t clamped to 0
        let (_lat, _lon, t) = project_on_segment(44.9, 4.9, 45.0, 5.0, 45.1, 5.1);
        assert!((t - 0.0).abs() < 1e-6);
    }

    #[test]
    fn project_on_polyline_finds_closest_segment() {
        // L-shaped route: (0,0) → (0,1) → (1,1)
        // Point at (0.1, 0.5) is closest to the first segment
        let route = vec![coord(45.0, 5.0), coord(45.0, 5.01), coord(45.01, 5.01)];
        let (perp, along) = project_on_polyline(&route, 45.001, 5.005);
        assert!(perp < 200.0, "perpendicular should be small, got {perp}");
        // Along should be roughly half the first segment (~500m)
        let total_first_seg = haversine_m(45.0, 5.0, 45.0, 5.01) / 1000.0;
        assert!(along < total_first_seg * 0.7, "along={along}");
        assert!(along > total_first_seg * 0.3, "along={along}");
    }

    #[test]
    fn build_overpass_query_contains_all_kinds() {
        let q = build_overpass_query(44.0, 44.5, 5.0, 5.5);
        assert!(q.contains("guest_house"));
        assert!(q.contains("hostel"));
        assert!(q.contains("alpine_hut"));
        // Rust f64 default formatting drops trailing .0 → "44,5,44.5,5.5"
        assert!(q.contains("44,5,44.5,5.5"), "bbox not in query: {q}");
        assert!(q.contains("out center tags"));
    }

    #[test]
    fn parse_element_node() {
        let el = serde_json::json!({
            "type": "node",
            "id": 12345,
            "lat": 44.27,
            "lon": 5.27,
            "tags": {
                "tourism": "guest_house",
                "name": "Le Soustet",
                "phone": "+33 4 75 28 16 32",
                "capacity": "12"
            }
        });
        let l = parse_element(&el).expect("should parse");
        assert_eq!(l.osm_id, 12345);
        assert_eq!(l.name, "Le Soustet");
        assert_eq!(l.kind, "guest_house");
        assert_eq!(l.phone.as_deref(), Some("+33 4 75 28 16 32"));
        assert_eq!(l.capacity.as_deref(), Some("12"));
    }

    #[test]
    fn parse_element_way_with_center() {
        let el = serde_json::json!({
            "type": "way",
            "id": 99,
            "center": { "lat": 44.27, "lon": 5.27 },
            "tags": { "tourism": "alpine_hut", "name": "Refuge X" }
        });
        let l = parse_element(&el).expect("should parse");
        assert_eq!(l.osm_type, "way");
        assert_eq!(l.lat, 44.27);
        assert_eq!(l.kind, "alpine_hut");
    }

    #[test]
    fn parse_element_missing_tourism_tag_returns_none() {
        let el = serde_json::json!({
            "type": "node",
            "id": 1,
            "lat": 44.0,
            "lon": 5.0,
            "tags": { "name": "Foo" }
        });
        assert!(parse_element(&el).is_none());
    }

    #[test]
    fn parse_element_address_concatenation() {
        let el = serde_json::json!({
            "type": "node",
            "id": 1,
            "lat": 44.0,
            "lon": 5.0,
            "tags": {
                "tourism": "hotel",
                "name": "Hôtel des Alpes",
                "addr:housenumber": "12",
                "addr:street": "rue de la Place",
                "addr:postcode": "26110",
                "addr:city": "Nyons"
            }
        });
        let l = parse_element(&el).expect("should parse");
        assert_eq!(l.address.as_deref(), Some("12 rue de la Place 26110 Nyons"));
    }
}
