//! Note un tracé déjà existant avec les poids du routeur.
//!
//! `hybridize` savait déjà le faire, mais seulement en chemin de son travail,
//! et il n'en imprime que des totaux par ancre. Pour relire un itinéraire
//! enregistré — vérifier sa part de forêt, voir ce qu'un changement de poids
//! lui ferait — il n'existait rien.
//!
//! Usage :
//!   score_route --input trace.json \
//!     [--w-pop 1.0] [--w-paved 2.0] [--w-forest 0.5] \
//!     [--pbf backend/data/troncais.osm.pbf] [--cache backend/data/cache] \
//!     [--margin-km 8] [--json]
//!
//! `trace.json` accepte les deux formes rencontrées dans le projet : le tableau
//! de paires `[lat, lon]` que lit `hybridize`, et le tableau d'objets
//! `{"lat":…, "lon":…}` que `route_data->'path'` rend en base.

use std::path::PathBuf;

use backend::{
    engine::{RouteEngine, WeightConfig},
    graph::{BoundingBox, GraphBuilder, GraphBuilderConfig, GraphFile},
    models::Coordinate,
};
use serde::Deserialize;

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

#[derive(Deserialize)]
#[serde(untagged)]
enum PointIn {
    Pair([f64; 2]),
    Object { lat: f64, lon: f64 },
}

impl From<PointIn> for Coordinate {
    fn from(p: PointIn) -> Self {
        match p {
            PointIn::Pair([lat, lon]) => Coordinate { lat, lon },
            PointIn::Object { lat, lon } => Coordinate { lat, lon },
        }
    }
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

/// Même clé de cache que `hybridize`, pour partager ses graphes : les deux
/// outils veulent exactement le même contenu, occupation du sol comprise.
fn engine_for(bbox: BoundingBox, pbf: &PathBuf, cache_dir: &PathBuf) -> Result<RouteEngine, String> {
    let key = bbox.cache_key();
    let cache_path = cache_dir.join(format!("hybridize_v4_{}.bin", key));
    if cache_path.exists() {
        if let Ok(graph) = GraphFile::read_from_path(&cache_path) {
            eprintln!("  graphe : cache ({} nœuds)", graph.nodes.len());
            return engine_with_population(graph);
        }
    }

    eprintln!("  graphe : construction depuis le PBF (lent)…");
    let builder = GraphBuilder::new(GraphBuilderConfig { bbox: Some(bbox) });
    let mut graph = builder
        .build_from_pbf(pbf)
        .map_err(|e| format!("build_from_pbf: {}", e))?;
    eprintln!("  graphe : {} nœuds, {} arêtes", graph.nodes.len(), graph.edges.len());

    match backend::graph::load_or_build_landcover(pbf, cache_dir, &bbox, &key) {
        Ok(grid) => {
            eprintln!(
                "  occupation du sol : {} cellules boisées, {} bâties",
                grid.forest_cells(),
                grid.built_cells()
            );
            backend::graph::apply_landcover(&mut graph, &grid);
        }
        // Sans la couche, les parts de forêt valent zéro et le diraient sans le
        // dire : on le signale au lieu de rendre un chiffre qui semble bon.
        Err(err) => eprintln!("  occupation du sol INDISPONIBLE ({err}) — part de forêt sans objet"),
    }

    std::fs::create_dir_all(cache_dir).ok();
    graph.write_to_path(&cache_path).ok();
    engine_with_population(graph)
}

/// Densité Insee, comme le serveur (`attach_population`). Jamais en cache :
/// appliquée après lecture, sinon `density_pressure` reste à zéro.
/// `POPULATION_GRID` désigne le CSV de `scripts/build_population_grid.sh`.
fn engine_with_population(mut graph: GraphFile) -> Result<RouteEngine, String> {
    let path = std::env::var("POPULATION_GRID").unwrap_or_else(|_| "data/population/insee_200m.csv".to_string());
    match backend::population::PopulationGrid::load(std::path::Path::new(&path)) {
        Ok(grid) => {
            eprintln!("  population Insee : {} carreaux lus depuis {path}", grid.len());
            backend::graph::apply_population(&mut graph, &grid);
        }
        Err(err) => eprintln!("  population Insee INDISPONIBLE ({err}) — densité sans objet"),
    }
    RouteEngine::from_graph_file(graph).map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();

    let input = PathBuf::from(
        arg(&args, "--input").ok_or("--input <trace.json> est obligatoire")?,
    );
    let pbf = PathBuf::from(
        arg(&args, "--pbf").unwrap_or_else(|| "backend/data/gr31-region.osm.pbf".into()),
    );
    let cache_dir = PathBuf::from(arg(&args, "--cache").unwrap_or_else(|| "backend/data/cache".into()));
    let margin_km = arg_f64(&args, "--margin-km", 8.0);
    let as_json = args.iter().any(|a| a == "--json");

    let weights = WeightConfig {
        population: arg_f64(&args, "--w-pop", 1.0),
        paved: arg_f64(&args, "--w-paved", 2.0),
        forest: arg_f64(&args, "--w-forest", 0.5),
        density: arg_f64(&args, "--w-density", 0.0),
    };

    let raw = std::fs::read_to_string(&input).map_err(|e| format!("{}: {}", input.display(), e))?;
    let points: Vec<Coordinate> = serde_json::from_str::<Vec<PointIn>>(&raw)
        .map_err(|e| format!("{}: {}", input.display(), e))?
        .into_iter()
        .map(Coordinate::from)
        .collect();
    if points.len() < 2 {
        return Err(format!("{} : moins de deux points", input.display()));
    }
    eprintln!("{} points lus", points.len());

    let engine = engine_for(bbox_around(&points, margin_km), &pbf, &cache_dir)?;
    let score = engine.score_polyline(&points, weights);

    // Les parts sont rapportées à la longueur *accrochée* et non à la longueur
    // totale : un pas hors graphe n'est ni boisé ni goudronné, il est inconnu,
    // et le compter au dénominateur ferait passer l'ignorance pour du goudron.
    let mesure_km = score.length_km - score.unmatched_km;
    let part = |km: f64| if mesure_km > 0.0 { 100.0 * km / mesure_km } else { 0.0 };

    if as_json {
        println!(
            r#"{{"length_km":{:.3},"measured_km":{:.3},"unmatched_km":{:.3},"paved_km":{:.3},"trail_km":{:.3},"dirt_km":{:.3},"forest_km":{:.3},"built_km":{:.3},"density_km":{:.3},"cost":{:.3},"forest_pct":{:.1},"paved_pct":{:.1}}}"#,
            score.length_km, mesure_km, score.unmatched_km,
            score.paved_km, score.trail_km, score.dirt_km,
            score.forest_km, score.built_km, score.density_km, score.cost,
            part(score.forest_km), part(score.paved_km),
        );
    } else {
        println!("longueur totale    {:8.2} km", score.length_km);
        println!("  accrochée        {:8.2} km  ({:.1} %)", mesure_km, 100.0 * mesure_km / score.length_km);
        println!("  hors graphe      {:8.2} km  ({:.1} %)", score.unmatched_km, 100.0 * score.unmatched_km / score.length_km);
        println!();
        println!("sur la part accrochée :");
        println!("  forêt            {:8.2} km  ({:.1} %)", score.forest_km, part(score.forest_km));
        println!("  goudron          {:8.2} km  ({:.1} %)", score.paved_km, part(score.paved_km));
        println!("  chemin           {:8.2} km  ({:.1} %)", score.trail_km, part(score.trail_km));
        println!("  terre            {:8.2} km  ({:.1} %)", score.dirt_km, part(score.dirt_km));
        println!("  densité bâtie    {:8.2} km·hab", score.built_km);
        println!("  densité Insee    {:8.2} km·hab", score.density_km);
        println!();
        println!("coût pondéré       {:8.2}  (w_pop {}, w_paved {}, w_forest {}, w_density {})",
            score.cost, weights.population, weights.paved, weights.forest, weights.density);
    }

    Ok(())
}
