//! Recherche des forêts publiques par leur nom (BD TOPO de l'IGN).
//!
//! Sert à dire « passer par la forêt de Bellême » : le nom donne un polygone,
//! dont on tire un point intérieur qui devient un point de passage. Le poids
//! forêt du calcul fait ensuite rester le tracé sous les arbres.

use std::time::Duration;

use serde::{Deserialize, Serialize};

const WFS_URL: &str = "https://data.geopf.fr/wfs/ows";
const WFS_LAYER: &str = "BDTOPO_V3:foret_publique";
const WFS_TIMEOUT: Duration = Duration::from_secs(30);

/// Assez pour départager les homonymes, peu pour une liste de suggestions.
const MAX_RESULTS: usize = 15;

/// Une forêt publique trouvée, réduite à ce qu'il faut pour y faire passer un
/// tracé.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Forest {
    pub name: String,
    /// « Forêt domaniale », « Forêt communale »…
    pub nature: Option<String>,
    /// Point à l'intérieur de la plus grande parcelle.
    pub lat: f64,
    pub lon: f64,
    pub area_ha: f64,
    /// `[min_lon, min_lat, max_lon, max_lat]`, pour cadrer la carte.
    pub bbox: [f64; 4],
}

#[derive(Debug, thiserror::Error)]
pub enum ForestError {
    #[error("requête trop courte")]
    QueryTooShort,
    #[error("service IGN : {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Deserialize)]
struct FeatureCollection {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    geometry: Option<Geometry>,
    properties: Properties,
}

#[derive(Deserialize)]
#[serde(tag = "type", content = "coordinates")]
enum Geometry {
    Polygon(Vec<Vec<[f64; 2]>>),
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
}

#[derive(Deserialize)]
struct Properties {
    toponyme: Option<String>,
    nature: Option<String>,
}

/// Motif `ILIKE` tolérant aux accents.
///
/// Le service ne sait pas ignorer les accents : « belleme » ne trouverait pas
/// « Bellême ». Chaque voyelle ou `c` devient le joker `_`, qui accepte aussi
/// bien la lettre nue que sa forme accentuée. Les apostrophes sont doublées
/// pour rester dans la chaîne CQL ; les jokers tapés sont neutralisés.
pub fn ilike_pattern(query: &str) -> Option<String> {
    let query = query.trim();
    if query.chars().count() < 3 {
        return None;
    }
    let body: String = query
        .chars()
        .map(|c| match c.to_lowercase().next().unwrap_or(c) {
            'a' | 'à' | 'â' | 'ä' | 'e' | 'é' | 'è' | 'ê' | 'ë' | 'i' | 'î' | 'ï' | 'o' | 'ô' | 'ö' | 'u' | 'ù'
            | 'û' | 'ü' | 'c' | 'ç' | '%' => '_'.to_string(),
            '\'' => "''".to_string(),
            other => other.to_string(),
        })
        .collect();
    Some(format!("%{body}%"))
}

/// Aire d'un anneau en m², projection locale équirectangulaire.
fn ring_area_m2(ring: &[[f64; 2]]) -> f64 {
    let Some(first) = ring.first() else { return 0.0 };
    let kx = 111_320.0 * first[1].to_radians().cos();
    let ky = 110_540.0;
    let twice: f64 = ring
        .windows(2)
        .map(|w| (w[0][0] * kx) * (w[1][1] * ky) - (w[1][0] * kx) * (w[0][1] * ky))
        .sum();
    (twice / 2.0).abs()
}

fn contains(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    for w in ring.windows(2) {
        let ([x1, y1], [x2, y2]) = (w[0], w[1]);
        if (y1 > p[1]) != (y2 > p[1]) && p[0] < x1 + (p[1] - y1) * (x2 - x1) / (y2 - y1) {
            inside = !inside;
        }
    }
    inside
}

/// Point à l'intérieur d'un anneau.
///
/// Le barycentre des sommets tombe hors d'une forêt en croissant ou en U : on
/// se rabat alors sur le milieu du premier segment intérieur de l'horizontale
/// qui passe par lui.
fn interior_point(ring: &[[f64; 2]]) -> [f64; 2] {
    let n = ring.len().max(1) as f64;
    let cx = ring.iter().map(|p| p[0]).sum::<f64>() / n;
    let cy = ring.iter().map(|p| p[1]).sum::<f64>() / n;
    if contains(ring, [cx, cy]) {
        return [cx, cy];
    }
    let mut xs: Vec<f64> = ring
        .windows(2)
        .filter(|w| (w[0][1] > cy) != (w[1][1] > cy))
        .map(|w| w[0][0] + (cy - w[0][1]) * (w[1][0] - w[0][0]) / (w[1][1] - w[0][1]))
        .collect();
    xs.sort_by(f64::total_cmp);
    match xs.as_slice() {
        [a, b, ..] => [(a + b) / 2.0, cy],
        _ => [cx, cy],
    }
}

fn to_forest(feature: Feature) -> Option<Forest> {
    // Anneaux extérieurs seulement : les trous (clairières, enclaves) ne
    // changent ni le cadrage ni le choix de la plus grande parcelle.
    let outers: Vec<Vec<[f64; 2]>> = match feature.geometry? {
        Geometry::Polygon(rings) => rings.into_iter().take(1).collect(),
        Geometry::MultiPolygon(polys) => polys.into_iter().filter_map(|r| r.into_iter().next()).collect(),
    };
    let biggest = outers.iter().max_by(|a, b| ring_area_m2(a).total_cmp(&ring_area_m2(b)))?;
    let [lon, lat] = interior_point(biggest);

    let points = outers.iter().flatten();
    let bbox = points.fold(
        [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
        |[a, b, c, d], p| [a.min(p[0]), b.min(p[1]), c.max(p[0]), d.max(p[1])],
    );

    Some(Forest {
        name: feature.properties.toponyme?,
        nature: feature.properties.nature,
        lat,
        lon,
        area_ha: outers.iter().map(|r| ring_area_m2(r)).sum::<f64>() / 10_000.0,
        bbox,
    })
}

/// Les domaniales d'abord : ce sont les forêts réputées (Bercé, Tronçais,
/// Fontainebleau…), l'IGN ne disant rien de la notoriété.
fn is_domaniale(forest: &Forest) -> bool {
    forest.nature.as_deref().is_some_and(|n| fold(n).contains("domaniale"))
}

/// Forêts d'une réponse GeoJSON : domaniales d'abord, puis les plus grandes.
fn parse(collection: FeatureCollection) -> Vec<Forest> {
    let mut forests: Vec<Forest> = collection.features.into_iter().filter_map(to_forest).collect();
    forests.sort_by(|a, b| is_domaniale(b).cmp(&is_domaniale(a)).then(b.area_ha.total_cmp(&a.area_ha)));
    forests.truncate(MAX_RESULTS);
    forests
}

/// Minuscules sans accents, pour comparer un nom à ce qui a été tapé.
pub fn fold(text: &str) -> String {
    text.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'à' | 'â' | 'ä' => "a".to_string(),
            'é' | 'è' | 'ê' | 'ë' => "e".to_string(),
            'î' | 'ï' => "i".to_string(),
            'ô' | 'ö' => "o".to_string(),
            'ù' | 'û' | 'ü' => "u".to_string(),
            'ç' => "c".to_string(),
            'œ' => "oe".to_string(),
            other => other.to_string(),
        })
        .collect()
}

/// Noms distincts qui contiennent vraiment `query`, accents ignorés.
///
/// Le motif `ILIKE` remplace chaque voyelle par un joker et ramène des
/// centaines de faux amis (« bercé » → `%b_r__%` → Biros, Buremont…) : ce
/// second filtre ne garde que les vrais.
fn matching_names(names: impl IntoIterator<Item = String>, query: &str) -> Vec<String> {
    let wanted = fold(query.trim());
    let mut kept: Vec<String> = names.into_iter().filter(|name| fold(name).contains(&wanted)).collect();
    kept.sort();
    kept.dedup();
    kept
}

#[derive(Deserialize)]
struct NameCollection {
    features: Vec<NameFeature>,
}

#[derive(Deserialize)]
struct NameFeature {
    properties: NameProperties,
}

#[derive(Deserialize)]
struct NameProperties {
    toponyme: Option<String>,
}

fn wfs_get<T: serde::de::DeserializeOwned>(
    client: &reqwest::blocking::Client,
    filter: &str,
    properties: &str,
    count: &str,
) -> Result<T, ForestError> {
    Ok(client
        .get(WFS_URL)
        .query(&[
            ("SERVICE", "WFS"),
            ("VERSION", "2.0.0"),
            ("REQUEST", "GetFeature"),
            ("TYPENAMES", WFS_LAYER),
            ("SRSNAME", "urn:ogc:def:crs:OGC:1.3:CRS84"),
            ("CQL_FILTER", filter),
            ("PROPERTYNAME", properties),
            ("COUNT", count),
            ("outputFormat", "application/json"),
        ])
        .send()?
        .error_for_status()?
        .json()?)
}

/// Cherche les forêts publiques dont le nom contient `query`.
///
/// Deux requêtes : les noms seuls d'abord, légers, filtrés ici sans accents ;
/// puis la géométrie des seuls noms retenus.
pub fn search(query: &str) -> Result<Vec<Forest>, ForestError> {
    let pattern = ilike_pattern(query).ok_or(ForestError::QueryTooShort)?;
    let client = reqwest::blocking::Client::builder().timeout(WFS_TIMEOUT).build()?;

    let names: NameCollection = wfs_get(&client, &format!("toponyme ILIKE '{pattern}'"), "toponyme", "1000")?;
    let mut names = matching_names(names.features.into_iter().filter_map(|f| f.properties.toponyme), query);
    if names.is_empty() {
        return Ok(Vec::new());
    }
    // Au-delà, la liste ne se lit plus et l'URL s'allonge pour rien.
    names.truncate(4 * MAX_RESULTS);

    let quoted: Vec<String> = names.iter().map(|n| format!("'{}'", n.replace('\'', "''"))).collect();
    let filter = format!("toponyme IN ({})", quoted.join(","));
    let collection: FeatureCollection = wfs_get(&client, &filter, "geometrie,toponyme,nature", "200")?;
    Ok(parse(collection))
}

#[derive(Deserialize)]
pub struct ForestQuery {
    pub q: String,
}

/// GET /api/forests?q=bellême
pub async fn search_handler(
    axum::extract::Query(query): axum::extract::Query<ForestQuery>,
) -> Result<axum::Json<Vec<Forest>>, (axum::http::StatusCode, String)> {
    use axum::http::StatusCode;
    tokio::task::spawn_blocking(move || search(&query.q))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Task error: {e}")))?
        .map(axum::Json)
        .map_err(|e| match e {
            ForestError::QueryTooShort => (StatusCode::BAD_REQUEST, e.to_string()),
            ForestError::Http(_) => (StatusCode::BAD_GATEWAY, e.to_string()),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_matches_accented_and_bare_spellings() {
        assert_eq!(ilike_pattern("Belleme").as_deref(), Some("%b_ll_m_%"));
        assert_eq!(ilike_pattern("bellême").as_deref(), Some("%b_ll_m_%"));
    }

    #[test]
    fn pattern_keeps_quotes_inside_the_cql_string() {
        assert_eq!(ilike_pattern("l'Hôpital").as_deref(), Some("%l''h_p_t_l%"));
    }

    #[test]
    fn only_names_really_containing_the_query_are_kept() {
        let names = ["Forêt Domaniale de Bercé", "Forêt Domaniale du Biros", "Forêt Domaniale de Buremont", "Forêt Domaniale de Bercé"]
            .map(String::from);
        assert_eq!(matching_names(names, "berce"), vec!["Forêt Domaniale de Bercé".to_string()]);
    }

    #[test]
    fn fold_drops_accents_and_case() {
        assert_eq!(fold("Bellême Œuvre Ç"), "belleme oeuvre c");
    }

    #[test]
    fn pattern_rejects_short_queries() {
        assert_eq!(ilike_pattern(" ab "), None);
    }

    #[test]
    fn interior_point_of_a_u_shape_stays_inside() {
        // U ouvert vers le haut : le barycentre tombe dans le creux.
        let u = vec![
            [0.0, 0.0], [3.0, 0.0], [3.0, 3.0], [2.0, 3.0], [2.0, 1.0],
            [1.0, 1.0], [1.0, 3.0], [0.0, 3.0], [0.0, 0.0],
        ];
        assert!(contains(&u, interior_point(&u)));
    }

    #[test]
    fn domaniales_first_then_biggest_parcel_wins() {
        let json = r#"{"features":[
          {"geometry":{"type":"Polygon","coordinates":[[[0,48],[0.01,48],[0.01,48.01],[0,48.01],[0,48]]]},
           "properties":{"toponyme":"Petite","nature":"Forêt communale"}},
          {"geometry":{"type":"Polygon","coordinates":[[[3,48],[3.005,48],[3.005,48.005],[3,48.005],[3,48]]]},
           "properties":{"toponyme":"Réputée","nature":"Forêt domaniale"}},
          {"geometry":{"type":"MultiPolygon","coordinates":[
             [[[1,48],[1.001,48],[1.001,48.001],[1,48.001],[1,48]]],
             [[[2,48],[2.1,48],[2.1,48.1],[2,48.1],[2,48]]]]},
           "properties":{"toponyme":"Grande","nature":"Forêt domaniale"}}]}"#;
        let forests = parse(serde_json::from_str(json).unwrap());
        // « Réputée », domaniale, passe devant la communale pourtant plus grande.
        assert_eq!(forests.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Grande", "Réputée", "Petite"]);
        assert_eq!(forests[0].name, "Grande");
        assert!((forests[0].lon - 2.04).abs() < 0.05 && (forests[0].lat - 48.04).abs() < 0.05);
        assert_eq!(forests[0].bbox, [1.0, 48.0, 2.1, 48.1]);
    }
}
