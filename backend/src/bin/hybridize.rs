//! Blend a hand-made trace (a GR, a GPX import) with the chemins-noirs router.
//!
//! The trace is cut into anchors every `--anchor-km`. For each pair of anchors
//! the router proposes an alternative; it replaces that stretch only when it is
//! both cheaper (weighted cost) and no longer than the original by more than
//! `--tolerance`. Everywhere else the original trace wins, so the result still
//! follows the waymarked path except where a darker way exists nearby.
//!
//! Usage:
//!   hybridize --input trace.json --output hybrid.json \
//!     [--anchor-km 3] [--tolerance 0.15] [--w-pop 1.0] [--w-paved 3.0] \
//!     [--pbf data/gr31-region.osm.pbf] [--cache data/cache] [--chunk-km 30]
//!
//! `trace.json` is a JSON array of [lat, lon] pairs; the output keeps that shape
//! and adds the per-anchor decisions.

use std::path::PathBuf;

use backend::{
    engine::{PolylineScore, RouteEngine, WeightConfig},
    geo_utils::fold_overlap,
    graph::{BoundingBox, GraphBuilder, GraphBuilderConfig, GraphFile},
    models::{Coordinate, RouteRequest},
    routing::haversine_km,
};
use serde::Serialize;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn arg_f64(args: &[String], name: &str, default: f64) -> f64 {
    arg(args, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[derive(Serialize)]
struct AnchorDecision {
    index: usize,
    from_index: usize,
    to_index: usize,
    original_km: f64,
    original_cost: f64,
    routed_km: Option<f64>,
    routed_cost: Option<f64>,
    original_paved_km: f64,
    original_unmatched_km: f64,
    original_forest_km: f64,
    routed_paved_km: Option<f64>,
    routed_forest_km: Option<f64>,
    replaced: bool,
    reason: String,
}

#[derive(Serialize)]
struct Output {
    path: Vec<[f64; 2]>,
    anchors: Vec<AnchorDecision>,
    original_km: f64,
    hybrid_km: f64,
    replaced_count: usize,
    replaced_km: f64,
    /// Totaux du GR d'origine et du tracé retenu, pour comparer d'un run à
    /// l'autre sans avoir à re-scorer les fichiers de sortie.
    original_paved_km: f64,
    original_forest_km: f64,
    hybrid_paved_km: f64,
    hybrid_forest_km: f64,
}

/// Anchor indices every `step_km` along the trace, always keeping both ends.
fn anchor_indices(path: &[Coordinate], step_km: f64) -> Vec<usize> {
    let mut anchors = vec![0usize];
    let mut acc = 0.0;
    for i in 1..path.len() {
        acc += haversine_km(path[i - 1], path[i]);
        if acc >= step_km {
            anchors.push(i);
            acc = 0.0;
        }
    }
    if *anchors.last().unwrap() != path.len() - 1 {
        anchors.push(path.len() - 1);
    }
    anchors
}

fn bbox_around(points: &[Coordinate], margin_km: f64) -> BoundingBox {
    let (mut min_lat, mut max_lat) = (f64::MAX, f64::MIN);
    let (mut min_lon, mut max_lon) = (f64::MAX, f64::MIN);
    for p in points {
        min_lat = min_lat.min(p.lat);
        max_lat = max_lat.max(p.lat);
        min_lon = min_lon.min(p.lon);
        max_lon = max_lon.max(p.lon);
    }
    let lat_margin = margin_km / 111.0;
    let cos_lat = ((min_lat + max_lat) / 2.0).to_radians().cos().abs().max(0.1);
    let lon_margin = margin_km / (111.0 * cos_lat);
    BoundingBox {
        min_lat: min_lat - lat_margin,
        max_lat: max_lat + lat_margin,
        min_lon: min_lon - lon_margin,
        max_lon: max_lon + lon_margin,
    }
}

/// Build (or reuse from disk) the graph for one chunk of the trace.
fn engine_for(bbox: BoundingBox, pbf: &PathBuf, cache_dir: &PathBuf) -> Result<RouteEngine, String> {
    let key = bbox.cache_key();
    // `v2` : les graphes d'avant la couche d'occupation du sol n'ont ni part
    // boisée ni densité bâtie, et se reliraient en silence avec des zéros.
    let cache_path = cache_dir.join(format!("hybridize_v2_{}.bin", key));
    if cache_path.exists() {
        if let Ok(graph) = GraphFile::read_from_path(&cache_path) {
            eprintln!("  graph: cache hit ({} nodes)", graph.nodes.len());
            return RouteEngine::from_graph_file(graph).map_err(|e| e.to_string());
        }
    }

    eprintln!("  graph: building from PBF (slow)…");
    let builder = GraphBuilder::new(GraphBuilderConfig { bbox: Some(bbox) });
    let mut graph = builder
        .build_from_pbf(pbf)
        .map_err(|e| format!("build_from_pbf: {}", e))?;
    eprintln!("  graph: {} nodes, {} edges", graph.nodes.len(), graph.edges.len());

    match backend::graph::load_or_build_landcover(pbf, cache_dir, &bbox, &key) {
        Ok(grid) => {
            eprintln!(
                "  landcover: {} cellules boisées, {} bâties",
                grid.forest_cells(),
                grid.built_cells()
            );
            backend::graph::apply_landcover(&mut graph, &grid);
        }
        // Sans la couche, `--w-forest` et `--w-pop` n'ont plus de prise : on le
        // dit fort plutôt que de rendre des chiffres qui semblent bons.
        Err(err) => eprintln!("  landcover INDISPONIBLE ({err}) — w-forest et w-pop sans effet"),
    }

    std::fs::create_dir_all(cache_dir).ok();
    graph.write_to_path(&cache_path).ok();
    RouteEngine::from_graph_file(graph).map_err(|e| e.to_string())
}

fn polyline_km(points: &[Coordinate]) -> f64 {
    points.windows(2).map(|p| haversine_km(p[0], p[1])).sum()
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let input = arg(&args, "--input").ok_or("--input is required")?;
    let output = arg(&args, "--output").ok_or("--output is required")?;
    let anchor_km = arg_f64(&args, "--anchor-km", 3.0);
    let tolerance = arg_f64(&args, "--tolerance", 0.15);
    let chunk_km = arg_f64(&args, "--chunk-km", 30.0);
    // A routed alternative starts and ends at the nearest routable node, which
    // can sit far from the trace where the GR runs off-graph. Splicing that in
    // would tear the line open, so such an alternative is refused.
    let max_snap_m = arg_f64(&args, "--max-snap-m", 60.0);
    let weights = WeightConfig {
        population: arg_f64(&args, "--w-pop", 1.0),
        paved: arg_f64(&args, "--w-paved", 3.0),
        forest: arg_f64(&args, "--w-forest", 0.0),
    };
    let pbf = PathBuf::from(arg(&args, "--pbf").unwrap_or_else(|| "data/gr31-region.osm.pbf".into()));
    let cache_dir = PathBuf::from(arg(&args, "--cache").unwrap_or_else(|| "data/cache".into()));

    let raw: Vec<[f64; 2]> = serde_json::from_str(
        &std::fs::read_to_string(&input).map_err(|e| format!("read {}: {}", input, e))?,
    )
    .map_err(|e| format!("parse {}: {}", input, e))?;
    let path: Vec<Coordinate> = raw.iter().map(|p| Coordinate { lat: p[0], lon: p[1] }).collect();

    let anchors = anchor_indices(&path, anchor_km);
    let original_km = polyline_km(&path);
    eprintln!(
        "{} points, {:.1} km, {} anchors every ~{} km",
        path.len(),
        original_km,
        anchors.len(),
        anchor_km
    );

    // Anchors are grouped into chunks so one graph serves many comparisons.
    let per_chunk = ((chunk_km / anchor_km).round() as usize).max(1);

    let mut hybrid: Vec<Coordinate> = vec![path[0]];
    let mut decisions = Vec::new();
    let mut replaced_count = 0usize;
    let mut replaced_km = 0.0;
    let mut original_paved_km = 0.0;
    let mut original_forest_km = 0.0;
    let mut hybrid_paved_km = 0.0;
    let mut hybrid_forest_km = 0.0;

    for (chunk_no, chunk) in anchors.windows(2).collect::<Vec<_>>().chunks(per_chunk).enumerate() {
        let first = chunk.first().unwrap()[0];
        let last = chunk.last().unwrap()[1];
        eprintln!(
            "chunk {} — anchors {}..{} ({:.1} km)",
            chunk_no + 1,
            first,
            last,
            polyline_km(&path[first..=last])
        );
        let engine = engine_for(bbox_around(&path[first..=last], 8.0), &pbf, &cache_dir)?;

        for pair in chunk {
            let (a, b) = (pair[0], pair[1]);
            let original = &path[a..=b];
            let original_score = engine.score_polyline(original, weights);
            let original_len = original_score.length_km;

            let routed = engine.find_path(&RouteRequest {
                start: path[a],
                end: path[b],
                w_pop: weights.population,
                w_paved: weights.paved,
                w_forest: weights.forest,
            });

            let (keep, routed_km, routed_cost, routed_paved_km, routed_forest_km, reason) = match routed {
                None => (None, None, None, None, None, "no route".to_string()),
                Some(alt) => {
                    let alt_score: PolylineScore = engine.score_polyline(&alt, weights);
                    let longer = alt_score.length_km > original_len * (1.0 + tolerance);
                    let cheaper = alt_score.cost < original_score.cost;
                    let snap_gap_m = haversine_km(path[a], *alt.first().unwrap()).max(
                        haversine_km(path[b], *alt.last().unwrap()),
                    ) * 1000.0;
                    let torn = snap_gap_m > max_snap_m;
                    let reason = if torn {
                        format!("{:.0} m gap at junction", snap_gap_m)
                    } else if longer {
                        format!("+{:.0}% too long", (alt_score.length_km / original_len - 1.0) * 100.0)
                    } else if !cheaper {
                        "not darker".to_string()
                    } else {
                        "darker".to_string()
                    };
                    let take = (!torn && !longer && cheaper).then_some(alt);
                    (
                        take,
                        Some(alt_score.length_km),
                        Some(alt_score.cost),
                        Some(alt_score.paved_km),
                        Some(alt_score.forest_km),
                        reason,
                    )
                }
            };

            original_paved_km += original_score.paved_km;
            original_forest_km += original_score.forest_km;
            if keep.is_some() {
                hybrid_paved_km += routed_paved_km.unwrap_or(0.0);
                hybrid_forest_km += routed_forest_km.unwrap_or(0.0);
            } else {
                hybrid_paved_km += original_score.paved_km;
                hybrid_forest_km += original_score.forest_km;
            }

            let replaced = keep.is_some();
            let chosen: Vec<Coordinate> = keep.unwrap_or_else(|| original.to_vec());
            if replaced {
                replaced_count += 1;
                replaced_km += polyline_km(&chosen);
            }
            // Coudre plutôt que concaténer : quand l'ancre tombe au bout d'une
            // branche du réseau, la portion précédente descend l'y chercher et
            // celle-ci remonte par le même chemin — une antenne parcourue deux
            // fois que ni l'une ni l'autre ne peut voir seule.
            let (drop_tail, skip_head) = fold_overlap(&hybrid, &chosen);
            hybrid.truncate(hybrid.len() - drop_tail);
            hybrid.extend(chosen.into_iter().skip(skip_head));

            decisions.push(AnchorDecision {
                index: decisions.len(),
                from_index: a,
                to_index: b,
                original_km: original_len,
                original_cost: original_score.cost,
                routed_km,
                routed_cost,
                original_paved_km: original_score.paved_km,
                original_unmatched_km: original_score.unmatched_km,
                original_forest_km: original_score.forest_km,
                routed_paved_km,
                routed_forest_km,
                replaced,
                reason,
            });
        }
    }

    let hybrid_km = polyline_km(&hybrid);
    let out = Output {
        path: hybrid.iter().map(|c| [c.lat, c.lon]).collect(),
        anchors: decisions,
        original_km,
        hybrid_km,
        replaced_count,
        replaced_km,
        original_paved_km,
        original_forest_km,
        hybrid_paved_km,
        hybrid_forest_km,
    };
    std::fs::write(&output, serde_json::to_string(&out).map_err(|e| e.to_string())?)
        .map_err(|e| format!("write {}: {}", output, e))?;

    eprintln!(
        "hybrid: {:.1} km ({:+.1} km), {} stretches replaced ({:.1} km) -> {}",
        hybrid_km,
        hybrid_km - original_km,
        replaced_count,
        replaced_km,
        output
    );
    eprintln!(
        "  goudron {:.1} km ({:.1} %) <- {:.1} km ({:.1} %) | forêt {:.1} km ({:.1} %) <- {:.1} km ({:.1} %)",
        hybrid_paved_km,
        hybrid_paved_km / hybrid_km.max(1e-9) * 100.0,
        original_paved_km,
        original_paved_km / original_km.max(1e-9) * 100.0,
        hybrid_forest_km,
        hybrid_forest_km / hybrid_km.max(1e-9) * 100.0,
        original_forest_km,
        original_forest_km / original_km.max(1e-9) * 100.0,
    );
    Ok(())
}
