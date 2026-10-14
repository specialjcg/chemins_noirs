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

/// Tourism tags we consider as valid lodgings for hikers. A request picks a
/// subset; anything else is dropped, which also keeps free text out of the
/// Overpass query.
const TOURISM_KINDS: &[&str] = &[
    "guest_house",
    "hostel",
    "alpine_hut",
    "wilderness_hut",
    "chalet",
    "hotel",
    "motel",
    "apartment",
    "camp_site",
];

/// Known kinds among those requested; none requested (or none known) means all.
pub fn select_kinds(requested: Option<&[String]>) -> Vec<&'static str> {
    let selected: Vec<&'static str> = TOURISM_KINDS
        .iter()
        .copied()
        .filter(|k| requested.is_some_and(|r| r.iter().any(|x| x == k)))
        .collect();
    if selected.is_empty() {
        TOURISM_KINDS.to_vec()
    } else {
        selected
    }
}

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
    /// OSM `tourism=*` values to keep; all known kinds when absent.
    #[serde(default)]
    pub kinds: Option<Vec<String>>,
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
fn build_overpass_query(
    kinds: &[&str],
    min_lat: f64,
    max_lat: f64,
    min_lon: f64,
    max_lon: f64,
) -> String {
    let kinds = kinds.join("|");
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
    kinds: &[&str],
    min_lat: f64,
    max_lat: f64,
    min_lon: f64,
    max_lon: f64,
) -> Result<serde_json::Value, String> {
    let query = build_overpass_query(kinds, min_lat, max_lat, min_lon, max_lon);

    // Try main endpoint then Kumi mirror — Overpass main is often saturated.
    let endpoints = [
        "https://overpass-api.de/api/interpreter",
        "https://overpass.kumi.systems/api/interpreter",
    ];

    // Both Overpass servers reject anonymous clients (HTTP 406 / 429).
    let client = reqwest::Client::builder()
        .user_agent(concat!("chemins-noirs/", env!("CARGO_PKG_VERSION"), " (hiking route planner)"))
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
    kinds: &[&str],
) -> Result<LodgingsResponse, String> {
    if coords.len() < 2 {
        return Err("coords must contain at least 2 points".to_string());
    }

    // 1. Compute expanded bbox
    let (min_lat, max_lat, min_lon, max_lon) = expanded_bbox(coords, buffer_m);

    // 2. Query Overpass
    let raw = fetch_overpass(kinds, min_lat, max_lat, min_lon, max_lon).await?;
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

// ─── Stage Planner ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PlanStagesRequest {
    pub lodgings: Vec<Lodging>,
    pub total_route_km: f64,
    pub target_km_per_day: f64,
    pub min_km_per_day: f64,
    pub max_km_per_day: f64,
    /// First and last points of the route: lodgings for the night before
    /// leaving and the night after arriving are looked for around them.
    #[serde(default)]
    pub start: Option<Coordinate>,
    #[serde(default)]
    pub end: Option<Coordinate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stage {
    pub day: usize,
    pub start_km: f64,
    pub end_km: f64,
    pub distance_km: f64,
    pub lodging: Option<Lodging>,
    pub lodging_score: f64,
    pub alternatives: Vec<Lodging>,
    /// False when no lodging allowed a stage within [min, max] km here.
    pub within_range: bool,
}

#[derive(Debug, Serialize)]
pub struct PlanStagesResponse {
    pub stages: Vec<Stage>,
    pub num_stages: usize,
    pub avg_km_per_day: f64,
    /// Night before the first day, near the start.
    pub departure: Option<EndpointLodging>,
    /// Night after the last day, near the arrival.
    pub arrival: Option<EndpointLodging>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndpointLodging {
    pub lodging: Lodging,
    /// Straight-line distance from the route's end point, in meters.
    pub distance_m: f64,
    pub alternatives: Vec<Lodging>,
}

/// Furthest an endpoint lodging may be from the start or the arrival.
const ENDPOINT_RADIUS_M: f64 = 2000.0;

/// Closest lodgings to a route end; the score only breaks near-ties (~200 m).
fn endpoint_lodging(lodgings: &[Lodging], at: Option<Coordinate>) -> Option<EndpointLodging> {
    let at = at?;
    let mut near: Vec<(f64, &Lodging)> = lodgings
        .iter()
        .map(|l| (haversine_m(at.lat, at.lon, l.lat, l.lon), l))
        .filter(|(d, _)| *d <= ENDPOINT_RADIUS_M)
        .collect();
    near.sort_by(|a, b| {
        let rank = |(d, l): &(f64, &Lodging)| d - 2.0 * score_lodging(l);
        rank(a).total_cmp(&rank(b))
    });
    let mut near = near.into_iter();
    let (distance_m, lodging) = near.next()?;
    Some(EndpointLodging {
        lodging: lodging.clone(),
        distance_m,
        alternatives: near.take(3).map(|(_, l)| l.clone()).collect(),
    })
}

fn score_lodging(l: &Lodging) -> f64 {
    let kind_score = if matches!(l.kind.as_str(), "guest_house" | "hostel" | "alpine_hut") {
        30.0
    } else {
        15.0
    };
    let phone_score = if l.phone.is_some() { 20.0 } else { 0.0 };
    let website_score = if l.website.is_some() { 15.0 } else { 0.0 };
    let capacity_score = if l.capacity.is_some() { 10.0 } else { 0.0 };
    let proximity_score = 25.0 * (1.0 - l.perpendicular_dist_m / 2000.0).max(0.0);
    kind_score + phone_score + website_score + capacity_score + proximity_score
}

/// Penalty per km outside [min, max]: large enough that any plan within range
/// beats one that is not, small enough to still rank impossible stretches.
const OUT_OF_RANGE_PENALTY: f64 = 1000.0;

/// A place where a day can end: the start, a lodging, or the arrival.
struct Cut<'a> {
    along_km: f64,
    detour_km: f64,
    lodging: Option<&'a Lodging>,
}

/// Walking distance between two cuts: the stretch of route, plus the detour to
/// leave the morning lodging and to reach the evening one.
fn stage_km(from: &Cut, to: &Cut) -> f64 {
    to.along_km - from.along_km + from.detour_km + to.detour_km
}

fn stage_cost(km: f64, req: &PlanStagesRequest) -> f64 {
    let outside = (req.min_km_per_day - km).max(0.0) + (km - req.max_km_per_day).max(0.0);
    (km - req.target_km_per_day).powi(2) + OUT_OF_RANGE_PENALTY * outside
}

fn within_range(km: f64, req: &PlanStagesRequest) -> bool {
    km >= req.min_km_per_day && km <= req.max_km_per_day
}

/// Chooses the whole sequence of nights at once (shortest path over the cuts
/// sorted along the route), so one awkward stretch cannot push later days out
/// of range the way a day-by-day greedy choice did.
pub fn plan_stages(req: &PlanStagesRequest) -> PlanStagesResponse {
    let mut cuts = vec![Cut { along_km: 0.0, detour_km: 0.0, lodging: None }];
    let mut on_route: Vec<&Lodging> = req
        .lodgings
        .iter()
        .filter(|l| l.along_route_km > 0.0 && l.along_route_km < req.total_route_km)
        .collect();
    on_route.sort_by(|a, b| a.along_route_km.total_cmp(&b.along_route_km));
    cuts.extend(on_route.into_iter().map(|l| Cut {
        along_km: l.along_route_km,
        detour_km: l.perpendicular_dist_m / 1000.0,
        lodging: Some(l),
    }));
    cuts.push(Cut { along_km: req.total_route_km, detour_km: 0.0, lodging: None });

    let n = cuts.len();
    let mut best = vec![f64::INFINITY; n];
    let mut prev = vec![0usize; n];
    best[0] = 0.0;
    for j in 1..n {
        // Score only breaks ties between lodgings at similar distances.
        let bonus = cuts[j].lodging.map_or(0.0, |l| score_lodging(l) / 100.0);
        for i in 0..j {
            if cuts[j].along_km <= cuts[i].along_km {
                continue;
            }
            let cost = best[i] + stage_cost(stage_km(&cuts[i], &cuts[j]), req) - bonus;
            if cost < best[j] {
                best[j] = cost;
                prev[j] = i;
            }
        }
    }

    let mut path = vec![n - 1];
    while let Some(&j) = path.last() {
        if j == 0 {
            break;
        }
        path.push(prev[j]);
    }
    path.reverse();

    let stages: Vec<Stage> = path
        .windows(2)
        .enumerate()
        .map(|(day, w)| {
            let (from, to) = (&cuts[w[0]], &cuts[w[1]]);
            let km = stage_km(from, to);
            let mut alternatives: Vec<(f64, &Lodging)> = cuts[1..n - 1]
                .iter()
                .filter(|c| c.along_km > from.along_km)
                .filter_map(|c| {
                    let l = c.lodging?;
                    let alt_km = stage_km(from, c);
                    let same = to.lodging.is_some_and(|t| t.osm_id == l.osm_id);
                    (!same && within_range(alt_km, req))
                        .then(|| ((alt_km - req.target_km_per_day).abs(), l))
                })
                .collect();
            alternatives.sort_by(|a, b| a.0.total_cmp(&b.0));
            Stage {
                day: day + 1,
                start_km: from.along_km,
                end_km: to.along_km,
                distance_km: km,
                lodging: to.lodging.cloned(),
                lodging_score: to.lodging.map_or(0.0, score_lodging),
                alternatives: alternatives.into_iter().take(3).map(|(_, l)| l.clone()).collect(),
                within_range: within_range(km, req),
            }
        })
        .collect();

    let num_stages = stages.len();
    let avg_km_per_day = stages.iter().map(|s| s.distance_km).sum::<f64>() / num_stages as f64;

    PlanStagesResponse {
        stages,
        num_stages,
        avg_km_per_day,
        departure: endpoint_lodging(&req.lodgings, req.start),
        arrival: endpoint_lodging(&req.lodgings, req.end),
    }
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
        let q = build_overpass_query(TOURISM_KINDS, 44.0, 44.5, 5.0, 5.5);
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

    fn lodging_at(osm_id: i64, along_route_km: f64, perpendicular_dist_m: f64) -> Lodging {
        Lodging {
            osm_id,
            osm_type: "node".into(),
            name: format!("L{osm_id}"),
            kind: "guest_house".into(),
            lat: 45.0,
            lon: 5.0,
            phone: None,
            website: None,
            email: None,
            capacity: None,
            stars: None,
            address: None,
            perpendicular_dist_m,
            along_route_km,
        }
    }

    fn plan(lodgings: Vec<Lodging>, total: f64) -> PlanStagesResponse {
        plan_stages(&PlanStagesRequest {
            lodgings,
            total_route_km: total,
            target_km_per_day: 25.0,
            min_km_per_day: 20.0,
            max_km_per_day: 30.0,
            start: None,
            end: None,
        })
    }

    #[test]
    fn plan_stages_avoids_greedy_dead_end() {
        // Greedy takes the 25 km lodging, then finds nothing in [45, 55]:
        // starting with 22 km keeps every day within range.
        let lodgings = vec![lodging_at(1, 22.0, 0.0), lodging_at(2, 25.0, 0.0), lodging_at(3, 44.0, 0.0)];
        let p = plan(lodgings, 68.0);
        let ends: Vec<f64> = p.stages.iter().map(|s| s.end_km).collect();
        assert_eq!(ends, vec![22.0, 44.0, 68.0]);
        assert!(p.stages.iter().all(|s| s.within_range));
    }

    #[test]
    fn plan_stages_counts_detour_in_distance() {
        // 1 km off the route: the evening and the next morning both pay it.
        let p = plan(vec![lodging_at(1, 25.0, 1000.0)], 50.0);
        assert_eq!(p.num_stages, 2);
        assert!((p.stages[0].distance_km - 26.0).abs() < 1e-9);
        assert!((p.stages[1].distance_km - 26.0).abs() < 1e-9);
    }

    #[test]
    fn plan_stages_flags_stretch_without_lodging() {
        let p = plan(vec![lodging_at(1, 25.0, 0.0)], 90.0);
        assert!(p.stages.iter().any(|s| !s.within_range));
        assert_eq!(p.stages.last().unwrap().end_km, 90.0);
    }

    #[test]
    fn endpoint_lodging_picks_closest_within_radius() {
        let mut near = lodging_at(1, 0.5, 0.0);
        near.lat = 45.005; // ~560 m north
        let mut far = lodging_at(2, 0.1, 0.0);
        far.lat = 45.05; // ~5.6 km north
        let lodgings = vec![far, near];
        let found = endpoint_lodging(&lodgings, Some(coord(45.0, 5.0))).expect("near one");
        assert_eq!(found.lodging.osm_id, 1);
        assert!(found.alternatives.is_empty());
        assert!(endpoint_lodging(&lodgings, None).is_none());
    }

    #[test]
    fn select_kinds_keeps_known_kinds_only() {
        let asked = vec!["hotel".to_string(), "camp_site".to_string(), "x\"];out;".to_string()];
        assert_eq!(select_kinds(Some(&asked)), vec!["hotel", "camp_site"]);
        assert_eq!(select_kinds(None), TOURISM_KINDS.to_vec());
    }
}
