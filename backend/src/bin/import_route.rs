//! Trace -> étapes calées sur les villages -> altitudes IGN -> ligne `saved_routes`.
//!
//! ```text
//! import_route --line trace.json --name "..." \
//!     [--description "..."] [--tags a,b,c] [--target-km 35] \
//!     [--places data/places.json] [--insert | --update ID]
//! ```
//!
//! `--line` attend un JSON `[[lat, lon], ...]` — le champ `path` que produit
//! `hybridize`. `--places` attend une réponse Overpass contenant des nœuds
//! `place=city|town|village|hamlet` :
//!
//! ```text
//! [out:json];node["place"~"^(city|town|village|hamlet)$"](BBOX);out;
//! ```
//!
//! Tout le calcul passe par le moteur — profil d'altitude, temps, difficulté,
//! GPX — pour qu'une ligne écrite ici soit celle que le backend recalculerait.

use std::{collections::HashMap, fs, path::PathBuf};

use backend::{
    database::{Database, SaveRouteRequest},
    elevation::create_elevation_profile,
    gpx_export::encode_route_as_gpx_with_elevations,
    routing::{estimate_time_minutes, haversine_km, rate_difficulty},
};
use serde::Deserialize;
use shared::{Coordinate, RouteResponse, SegmentStats};

/// Une coupure d'étape doit tomber sur un lieu où l'on peut dormir. Le hameau
/// reste possible mais coûte cher, la ville ne coûte rien.
fn penalty(kind: &str) -> Option<f64> {
    match kind {
        "city" | "town" => Some(0.0),
        "village" => Some(400.0),
        "hamlet" => Some(2000.0),
        _ => None,
    }
}

/// Fenêtre de recherche autour de la distance visée.
const WINDOW_M: f64 = 5_000.0;

/// Un village à côté du tracé se rejoint à pied : sa distance d'accès ne compte
/// que pour moitié, sinon aucune étape ne tomberait jamais sur un village.
const ACCESS_WEIGHT: f64 = 0.5;

#[derive(Clone, Debug)]
struct Place {
    coord: Coordinate,
    name: String,
    penalty: f64,
}

#[derive(Deserialize)]
struct OverpassResponse {
    elements: Vec<OverpassNode>,
}

#[derive(Deserialize)]
struct OverpassNode {
    lat: f64,
    lon: f64,
    tags: HashMap<String, String>,
}

fn load_places(path: &PathBuf) -> Result<Vec<Place>, String> {
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("lieux illisibles ({}) : {e}", path.display()))?;
    let parsed: OverpassResponse =
        serde_json::from_str(&raw).map_err(|e| format!("lieux illisibles : {e}"))?;

    Ok(parsed
        .elements
        .into_iter()
        .filter_map(|node| {
            let name = node.tags.get("name")?.clone();
            let penalty = penalty(node.tags.get("place")?)?;
            Some(Place {
                coord: Coordinate { lat: node.lat, lon: node.lon },
                name,
                penalty,
            })
        })
        .collect())
}

/// Distances cumulées, en mètres, le long du tracé.
fn cumulative(path: &[Coordinate]) -> Vec<f64> {
    let mut cum = Vec::with_capacity(path.len());
    cum.push(0.0);
    for pair in path.windows(2) {
        let last = *cum.last().unwrap();
        cum.push(last + haversine_km(pair[0], pair[1]) * 1000.0);
    }
    cum
}

/// Indices de coupure, calés sur le meilleur lieu proche de chaque cible.
///
/// Le nombre d'étapes est fixé d'abord, puis à chaque coupure on répartit ce
/// qui reste sur les étapes restantes. Avancer par pas fixes laisse un résidu
/// à la fin ; viser des multiples absolus laisse les écarts s'additionner en
/// sens contraires, ce qui donne une étape de 29 km suivie d'une de 45.
fn cut_stages(
    path: &[Coordinate],
    cum: &[f64],
    target_m: f64,
    places: &[Place],
) -> (Vec<usize>, HashMap<usize, String>) {
    let total = *cum.last().unwrap_or(&0.0);
    let count = ((total / target_m).round() as usize).max(1);

    let mut cuts = vec![0usize];
    let mut names: HashMap<usize, String> = HashMap::new();

    for step in 1..count {
        let done = cum[*cuts.last().unwrap()];
        let target = done + (total - done) / (count - step + 1) as f64;

        let lo = cum.iter().position(|&c| c >= target - WINDOW_M).unwrap_or(0);
        let hi = cum
            .iter()
            .position(|&c| c > target + WINDOW_M)
            .unwrap_or(cum.len() - 1);
        if lo >= hi {
            continue;
        }

        let mut best: Option<(usize, &Place, f64)> = None;
        for place in places {
            let (idx, dist_m) = (lo..hi)
                .map(|i| (i, haversine_km(place.coord, path[i]) * 1000.0))
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .unwrap();

            if dist_m > WINDOW_M {
                continue;
            }

            let score =
                dist_m * ACCESS_WEIGHT + place.penalty + 0.35 * (cum[idx] - target).abs();
            if best.as_ref().is_none_or(|(_, _, b)| score < *b) {
                best = Some((idx, place, score));
            }
        }

        let mut idx = match best {
            Some((idx, place, _)) => {
                names.insert(idx, place.name.clone());
                idx
            }
            None => (lo..hi)
                .min_by(|&a, &b| {
                    (cum[a] - target)
                        .abs()
                        .partial_cmp(&(cum[b] - target).abs())
                        .unwrap()
                })
                .unwrap(),
        };

        // Une coupure qui n'avance pas boucle indéfiniment : on force le pas.
        let previous = *cuts.last().unwrap();
        if idx <= previous {
            idx = (previous + 1).min(path.len().saturating_sub(2));
        }
        cuts.push(idx);
    }

    cuts.push(path.len() - 1);
    (cuts, names)
}

/// Lieu à retenir pour nommer une extrémité d'étape.
///
/// Le plus proche n'est pas le plus utile : un hameau à 1,7 km l'emporterait
/// sur le bourg à 2,7 km, alors que c'est le bourg qu'on cherche pour dormir ou
/// descendre du train. On reprend donc le score des coupures.
fn nearest_name(point: Coordinate, places: &[Place]) -> String {
    places
        .iter()
        .min_by(|a, b| {
            let score = |p: &Place| {
                haversine_km(point, p.coord) * 1000.0 * ACCESS_WEIGHT + p.penalty
            };
            score(a).partial_cmp(&score(b)).unwrap()
        })
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "?".to_string())
}

fn ascent_descent(elevations: &[Option<f64>]) -> (f64, f64) {
    let mut up = 0.0;
    let mut down = 0.0;
    for window in elevations.windows(2) {
        if let (Some(a), Some(b)) = (window[0], window[1]) {
            let diff = b - a;
            if diff > 0.0 {
                up += diff;
            } else {
                down -= diff;
            }
        }
    }
    (up, down)
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn arg_f64(args: &[String], name: &str, fallback: f64) -> f64 {
    arg(args, name).and_then(|v| v.parse().ok()).unwrap_or(fallback)
}

#[tokio::main]
async fn main() -> Result<(), String> {
    tracing_subscriber::fmt().with_env_filter("warn").init();

    let args: Vec<String> = std::env::args().collect();
    let line_path = arg(&args, "--line").ok_or("--line est obligatoire")?;
    let name = arg(&args, "--name").ok_or("--name est obligatoire")?;
    let description = arg(&args, "--description");
    let tags: Vec<String> = arg(&args, "--tags")
        .map(|t| t.split(',').filter(|s| !s.is_empty()).map(String::from).collect())
        .unwrap_or_default();
    let target_km = arg_f64(&args, "--target-km", 35.0);
    let elevations_path = arg(&args, "--elevations");
    let places_path = PathBuf::from(
        arg(&args, "--places").unwrap_or_else(|| "data/places.json".into()),
    );
    let update_id: Option<i32> = arg(&args, "--update").and_then(|v| v.parse().ok());
    let insert = args.iter().any(|a| a == "--insert");

    let raw: Vec<[f64; 2]> = serde_json::from_str(
        &fs::read_to_string(&line_path).map_err(|e| format!("read {line_path}: {e}"))?,
    )
    .map_err(|e| format!("parse {line_path}: {e}"))?;
    let path: Vec<Coordinate> = raw
        .iter()
        .map(|p| Coordinate { lat: p[0], lon: p[1] })
        .collect();
    if path.len() < 2 {
        return Err("tracé trop court".into());
    }

    let places = load_places(&places_path)?;
    let cum = cumulative(&path);
    let (cuts, names) = cut_stages(&path, &cum, target_km * 1000.0, &places);

    eprintln!(
        "{} points, {:.1} km, {} étapes",
        path.len(),
        cum.last().unwrap() / 1000.0,
        cuts.len() - 1
    );

    // Altitudes IGN, lissage et cumuls : exactement ce que fait le backend.
    // `--elevations` rejoue un import sur un profil déjà connu, sans redemander
    // le réseau — utile pour reprendre un tracé sans en changer les altitudes.
    let profile = match &elevations_path {
        Some(file) => {
            let raw: Vec<Option<f64>> = serde_json::from_str(
                &fs::read_to_string(file).map_err(|e| format!("read {file}: {e}"))?,
            )
            .map_err(|e| format!("parse {file}: {e}"))?;

            if raw.len() != path.len() {
                return Err(format!(
                    "{} altitudes pour {} points",
                    raw.len(),
                    path.len()
                ));
            }

            let (total_ascent, total_descent) = ascent_descent(&raw);
            let known: Vec<f64> = raw.iter().flatten().copied().collect();
            shared::ElevationProfile {
                min_elevation: known.iter().cloned().reduce(f64::min),
                max_elevation: known.iter().cloned().reduce(f64::max),
                elevations: raw,
                total_ascent,
                total_descent,
            }
        }
        None => create_elevation_profile(&path)
            .await
            .map_err(|e| format!("altitudes : {e}"))?,
    };

    let segments: Vec<SegmentStats> = cuts
        .windows(2)
        .map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let (up, down) = ascent_descent(&profile.elevations[a..=b]);
            let distance_km = (cum[b] - cum[a]) / 1000.0;
            SegmentStats {
                from_index: a,
                to_index: b,
                distance_km: (distance_km * 100.0).round() / 100.0,
                ascent_m: (up * 10.0).round() / 10.0,
                descent_m: (down * 10.0).round() / 10.0,
                avg_slope_pct: if distance_km > 0.0 {
                    ((up / (distance_km * 1000.0) * 100.0) * 10.0).round() / 10.0
                } else {
                    0.0
                },
            }
        })
        .collect();

    let distance_km = cum.last().unwrap() / 1000.0;
    let difficulty = rate_difficulty(&profile.elevations, &path, profile.total_ascent);
    let minutes = estimate_time_minutes(distance_km, profile.total_ascent);
    let gpx_base64 = encode_route_as_gpx_with_elevations(&path, Some(&profile.elevations))
        .map_err(|e| format!("gpx : {e}"))?;

    eprintln!(
        "{:.1} km | D+{:.0} D-{:.0} | {} | {} étapes",
        distance_km,
        profile.total_ascent,
        profile.total_descent,
        difficulty,
        segments.len()
    );
    for (i, segment) in segments.iter().enumerate() {
        let label = |idx: usize| {
            names
                .get(&idx)
                .cloned()
                .unwrap_or_else(|| nearest_name(path[idx], &places))
        };
        eprintln!(
            "  {:02}  {:6.2} km  D+{:6.1}  {} → {}",
            i + 1,
            segment.distance_km,
            segment.ascent_m,
            label(segment.from_index),
            label(segment.to_index)
        );
    }

    let route = RouteResponse {
        path,
        distance_km,
        gpx_base64,
        metadata: None,
        elevation_profile: Some(profile),
        snapped_waypoints: None,
        estimated_time_minutes: Some(minutes),
        difficulty: Some(difficulty),
        surface_breakdown: None,
        segments: Some(segments),
        point_surfaces: None,
    };

    if !insert && update_id.is_none() {
        eprintln!("\n(ni --insert ni --update : rien écrit en base)");
        return Ok(());
    }

    let request = SaveRouteRequest {
        name,
        description,
        route,
        tags: Some(tags),
        original_waypoints: None,
    };

    let db = Database::new().await.map_err(|e| format!("base : {e}"))?;
    let saved = match update_id {
        Some(id) => db.update_route(id, request).await,
        None => db.save_route(request).await,
    }
    .map_err(|e| format!("écriture : {e}"))?;

    eprintln!(
        "id {} | {} | {:.2} km | {}",
        saved.id, saved.name, saved.distance_km, saved.updated_at
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ligne droite plein est, un point tous les 100 m.
    fn straight(km: f64) -> Vec<Coordinate> {
        let steps = (km * 10.0) as usize;
        (0..=steps)
            .map(|i| Coordinate {
                lat: 47.5,
                // ~100 m par pas à cette latitude.
                lon: 2.0 + i as f64 * 0.001330,
            })
            .collect()
    }

    fn place(lat: f64, lon: f64, name: &str, penalty: f64) -> Place {
        Place { coord: Coordinate { lat, lon }, name: name.into(), penalty }
    }

    #[test]
    fn stage_count_follows_the_target() {
        let path = straight(100.0);
        let cum = cumulative(&path);
        let (cuts, _) = cut_stages(&path, &cum, 35_000.0, &[]);

        // 100 / 35 arrondi à 3.
        assert_eq!(cuts.len() - 1, 3, "{cuts:?}");
        assert_eq!(cuts[0], 0);
        assert_eq!(*cuts.last().unwrap(), path.len() - 1);
    }

    #[test]
    fn stages_stay_even_with_no_place_to_snap_to() {
        // Sans lieu où se caler, les étapes doivent rester régulières : c'est
        // la répartition du reste qui l'assure, pas les lieux.
        let path = straight(248.0);
        let cum = cumulative(&path);
        let (cuts, _) = cut_stages(&path, &cum, 35_000.0, &[]);

        let lengths: Vec<f64> = cuts
            .windows(2)
            .map(|w| (cum[w[1]] - cum[w[0]]) / 1000.0)
            .collect();
        let shortest = lengths.iter().cloned().fold(f64::MAX, f64::min);
        let longest = lengths.iter().cloned().fold(0.0, f64::max);

        assert!(
            longest - shortest < 1.0,
            "étapes déséquilibrées : {lengths:?}"
        );
    }

    #[test]
    fn cuts_move_forward_and_stay_inside_the_path() {
        let path = straight(120.0);
        let cum = cumulative(&path);
        let (cuts, _) = cut_stages(&path, &cum, 35_000.0, &[]);

        for pair in cuts.windows(2) {
            assert!(pair[0] < pair[1], "coupures non croissantes : {cuts:?}");
        }
        assert!(cuts.iter().all(|&i| i < path.len()));
    }

    #[test]
    fn a_village_wins_over_a_closer_hamlet() {
        let path = straight(70.0);
        let cum = cumulative(&path);

        // Les deux sont dans la fenêtre autour des 35 km ; le hameau est plus
        // près du tracé, mais on veut le bourg.
        let places = vec![
            place(47.5045, 2.4655, "Le Hameau", 2000.0),
            place(47.5090, 2.4655, "Le Bourg", 400.0),
        ];

        let (_, names) = cut_stages(&path, &cum, 35_000.0, &places);
        assert!(
            names.values().any(|n| n == "Le Bourg"),
            "attendu Le Bourg, obtenu {names:?}"
        );
    }

    #[test]
    fn naming_an_endpoint_prefers_the_town() {
        let point = Coordinate { lat: 47.5, lon: 2.0 };
        let places = vec![
            // 1,7 km, hameau.
            place(47.5153, 2.0, "Les Baudons", 2000.0),
            // 2,7 km, bourg.
            place(47.5243, 2.0, "Neuvy-sur-Barangeon", 400.0),
        ];

        assert_eq!(nearest_name(point, &places), "Neuvy-sur-Barangeon");
    }
}
