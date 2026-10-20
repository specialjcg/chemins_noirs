//! Chemins IGN ajoutés au graphe OSM : le cas de Champ Louis.
//!
//! Au nord de Châtillon-sur-Seine, entre deux points d'un itinéraire réel, OSM
//! n'a aucune voie sur un bon kilomètre alors que la carte IGN y dessine
//! plusieurs chemins. Le calcul faisait un long détour ; le marcheur devait
//! tirer un tronçon direct.
//!
//! Requires: un graphe en cache dans data/cache couvrant la zone (construit
//! par backend_partial au premier calcul) et la réponse IGN enregistrée dans
//! tests/fixtures, pour ne pas dépendre du réseau.

use backend::{
    engine::RouteEngine,
    graph::GraphFile,
    ign,
    models::{Coordinate, RouteRequest},
};

const POINT_23: Coordinate = Coordinate { lat: 47.8388, lon: 4.7578 };
const POINT_24: Coordinate = Coordinate { lat: 47.83915, lon: 4.72996 };

/// Graphe en cache dont l'emprise, notée à côté de lui, couvre les deux points.
fn cached_graph_covering_champ_louis() -> GraphFile {
    for dir in ["data/cache", "../data/cache"] {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("bbox") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let v: Vec<f64> = text.trim().split(',').filter_map(|x| x.parse().ok()).collect();
            let [min_lat, max_lat, min_lon, max_lon] = v[..] else { continue };
            let covers = |c: Coordinate| {
                (min_lat + 0.02..=max_lat - 0.02).contains(&c.lat)
                    && (min_lon + 0.02..=max_lon - 0.02).contains(&c.lon)
            };
            if covers(POINT_23) && covers(POINT_24) {
                if let Ok(graph) = GraphFile::read_from_path(path.with_extension("bin")) {
                    return graph;
                }
            }
        }
    }
    panic!("aucun graphe en cache autour de Châtillon-sur-Seine : lancer un calcul dans la zone d'abord");
}

fn route_km(engine: &RouteEngine) -> Option<(f64, Vec<Coordinate>)> {
    let path = engine.find_path(&RouteRequest {
        start: POINT_23,
        end: POINT_24,
        w_pop: 1.0,
        w_paved: 1.0,
        w_forest: 0.0,
        w_density: 0.0,
    })?;
    let km = path.windows(2).map(|w| backend::geo_utils::haversine_km(w[0], w[1])).sum();
    Some((km, path))
}

#[test]
fn ign_paths_cross_champ_louis_where_osm_has_none() {
    let mut graph = cached_graph_covering_champ_louis();

    let osm_only = RouteEngine::from_graph_file(graph.clone()).unwrap();
    let detour_km = route_km(&osm_only).map_or(f64::INFINITY, |(km, _)| km);
    assert!(detour_km > 4.0, "OSM seul devrait faire le détour : {detour_km:.2} km");

    let json = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ign_champ_louis.geojson"))
        .unwrap();
    let troncons = ign::parse_feature_collection(&json).unwrap();
    let stats = ign::merge_into(&mut graph, &troncons);
    assert!(stats.added_edges > 0 && stats.split_edges > 0, "{stats:?}");

    let merged = RouteEngine::from_graph_file(graph).unwrap();
    let (km, path) = route_km(&merged).expect("un chemin par les tronçons IGN");
    // 2,1 km à vol d'oiseau.
    assert!(km < 3.0, "passage par Champ Louis attendu, {km:.2} km (OSM seul : {detour_km:.2} km)");
    let on_ign = merged.ign_along(&path).iter().filter(|&&ign| ign).count();
    assert!(on_ign > 0, "le tracé devrait emprunter des tronçons IGN");
}
