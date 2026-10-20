//! Chemins de la BD TOPO de l'IGN, ajoutés au graphe OSM là où il en manque.
//!
//! OSM ignore une bonne part des chemins ruraux que la carte IGN dessine : au
//! nord de Châtillon-sur-Seine, aucune voie OSM ne traverse Champ Louis alors
//! que l'IGN y compte une cinquantaine de tronçons. Le calcul faisait alors un
//! détour de plusieurs kilomètres, et le seul recours était un tronçon direct.
//!
//! Deux étapes :
//!
//! - `load_or_fetch` demande au service WFS de la Géoplateforme les tronçons
//!   de nature Chemin, Sentier, Route empierrée et Escalier de l'emprise, et
//!   les garde en cache à côté du graphe ;
//! - `merge_into` n'en ajoute que les morceaux absents d'OSM : un point IGN à
//!   moins de `COVERED_M` d'une voie OSM est tenu pour déjà cartographié. Les
//!   morceaux retenus sont raccordés aux voies OSM voisines, en coupant
//!   celles-ci au point de jonction : sans cette coupure, ils formaient des
//!   îlots que le calcul ne pouvait atteindre.
//!
//! La fusion est refaite à chaque chargement du graphe, jamais écrite dans son
//! cache : celui-ci reste le graphe d'OSM seul (voir `EdgeRecord::ign`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::geo_utils::haversine_km;
use crate::graph::{BoundingBox, EdgeRecord, GraphFile, NodeRecord};
use crate::models::{Coordinate, SurfaceType};

/// Un point IGN à moins de cette distance d'une voie OSM est déjà cartographié.
///
/// Les deux sources divergent de 5 à 15 m sur un même chemin : en deçà, un
/// chemin présent dans les deux serait ajouté une seconde fois à côté de
/// lui-même.
pub const COVERED_M: f64 = 20.0;

/// Distance maximale entre le bout d'un morceau IGN et la voie OSM à laquelle
/// on le raccorde. Un peu plus large que `COVERED_M`, pour qu'un morceau qui
/// quitte une voie OSM trouve toujours celle-ci.
pub const CONNECT_M: f64 = 25.0;

/// Pas d'échantillonnage des tronçons IGN pour le test de recouvrement : leurs
/// sommets peuvent être espacés de plusieurs centaines de mètres.
const SAMPLE_M: f64 = 10.0;

/// Un écart non couvert plus court que ceci n'est qu'un décalage entre les
/// deux sources le long d'un même chemin, pas un chemin manquant.
const MIN_GAP_M: f64 = 20.0;

/// Deux jonctions plus proches que ceci n'en font qu'une.
const SAME_NODE_M: f64 = 3.0;

/// Identifiants des nœuds créés par la fusion, au-dessus de ceux d'OSM
/// (environ 2^34 aujourd'hui).
const NODE_ID_BASE: u64 = 1 << 62;

const WFS_URL: &str = "https://data.geopf.fr/wfs/ows";
const WFS_LAYER: &str = "BDTOPO_V3:troncon_de_route";
/// Plafond du service par requête : au-delà, on pagine.
const WFS_PAGE: usize = 5000;
const WFS_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IgnNature {
    Chemin,
    Sentier,
    RouteEmpierree,
    Escalier,
}

impl IgnNature {
    fn from_label(label: &str) -> Option<Self> {
        match label {
            "Chemin" => Some(Self::Chemin),
            "Sentier" => Some(Self::Sentier),
            "Route empierrée" => Some(Self::RouteEmpierree),
            "Escalier" => Some(Self::Escalier),
            _ => None,
        }
    }

    fn surface(self) -> SurfaceType {
        match self {
            Self::Chemin | Self::RouteEmpierree => SurfaceType::Dirt,
            Self::Sentier | Self::Escalier => SurfaceType::Trail,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IgnTroncon {
    pub nature: IgnNature,
    pub line: Vec<Coordinate>,
}

#[derive(Debug, thiserror::Error)]
pub enum IgnError {
    #[error("service IGN : {0}")]
    Http(#[from] reqwest::Error),
    #[error("cache IGN : {0}")]
    Io(#[from] std::io::Error),
    #[error("réponse IGN illisible : {0}")]
    Decode(String),
    #[error("requête remplacée")]
    Cancelled,
}

fn cache_path(cache_dir: &Path, cache_key: &str) -> PathBuf {
    cache_dir.join(format!("ign-v1_{cache_key}.bin"))
}

/// Tronçons IGN de l'emprise, relus du cache ou demandés au service.
///
/// Un échec réseau n'est pas mis en cache : la prochaine construction de la
/// même emprise retentera.
pub fn load_or_fetch(
    cache_dir: &Path,
    bbox: &BoundingBox,
    cache_key: &str,
    cancel: &AtomicBool,
) -> Result<Vec<IgnTroncon>, IgnError> {
    let path = cache_path(cache_dir, cache_key);
    if let Ok(bytes) = std::fs::read(&path) {
        match postcard::from_bytes(&bytes) {
            Ok(troncons) => return Ok(troncons),
            Err(err) => tracing::warn!("cache IGN illisible, nouvelle demande : {} ({err})", path.display()),
        }
    }

    let troncons = fetch(bbox, cancel)?;
    let bytes = postcard::to_allocvec(&troncons).map_err(|e| IgnError::Decode(e.to_string()))?;
    std::fs::create_dir_all(cache_dir)?;
    std::fs::write(&path, bytes)?;
    Ok(troncons)
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
    LineString(Vec<Vec<f64>>),
    MultiLineString(Vec<Vec<Vec<f64>>>),
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct Properties {
    nature: Option<String>,
}

fn fetch(bbox: &BoundingBox, cancel: &AtomicBool) -> Result<Vec<IgnTroncon>, IgnError> {
    // `BBOX` dans le filtre et non en paramètre : le service refuse les deux
    // ensemble. CRS84 pour recevoir longitude puis latitude, comme en GeoJSON.
    let filter = format!(
        "BBOX(geometrie,{},{},{},{},'urn:ogc:def:crs:OGC:1.3:CRS84') \
         AND nature IN ('Chemin','Sentier','Route empierrée','Escalier') \
         AND fictif=false",
        bbox.min_lon, bbox.min_lat, bbox.max_lon, bbox.max_lat
    );
    let client = reqwest::blocking::Client::builder().timeout(WFS_TIMEOUT).build()?;

    let mut troncons = Vec::new();
    let mut start = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(IgnError::Cancelled);
        }
        let page: FeatureCollection = client
            .get(WFS_URL)
            .query(&[
                ("SERVICE", "WFS"),
                ("VERSION", "2.0.0"),
                ("REQUEST", "GetFeature"),
                ("TYPENAMES", WFS_LAYER),
                ("SRSNAME", "urn:ogc:def:crs:OGC:1.3:CRS84"),
                ("CQL_FILTER", filter.as_str()),
                ("PROPERTYNAME", "geometrie,nature"),
                // Un ordre stable, sans quoi les pages se recouvrent.
                ("SORTBY", "cleabs"),
                ("COUNT", &WFS_PAGE.to_string()),
                ("STARTINDEX", &start.to_string()),
                ("outputFormat", "application/json"),
            ])
            .send()?
            .error_for_status()?
            .json()?;

        let received = page.features.len();
        troncons.extend(page.features.into_iter().flat_map(to_troncons));
        tracing::info!("IGN : {} tronçons reçus (page à partir de {start})", received);
        if received < WFS_PAGE {
            return Ok(troncons);
        }
        start += WFS_PAGE;
    }
}

/// Tronçons d'une réponse GeoJSON du service, natures non retenues écartées.
pub fn parse_feature_collection(json: &str) -> Result<Vec<IgnTroncon>, IgnError> {
    let collection: FeatureCollection = serde_json::from_str(json).map_err(|e| IgnError::Decode(e.to_string()))?;
    Ok(collection.features.into_iter().flat_map(to_troncons).collect())
}

fn to_troncons(feature: Feature) -> Vec<IgnTroncon> {
    let Some(nature) = feature.properties.nature.as_deref().and_then(IgnNature::from_label) else {
        return Vec::new();
    };
    let lines = match feature.geometry {
        Some(Geometry::LineString(line)) => vec![line],
        Some(Geometry::MultiLineString(lines)) => lines,
        _ => Vec::new(),
    };
    lines
        .into_iter()
        .map(|line| IgnTroncon {
            nature,
            line: line
                .iter()
                .filter(|p| p.len() >= 2)
                .map(|p| Coordinate { lat: p[1], lon: p[0] })
                .collect(),
        })
        .filter(|t| t.line.len() >= 2)
        .collect()
}

/// Bilan d'une fusion, pour le journal.
#[derive(Debug, Default)]
pub struct MergeStats {
    pub added_edges: usize,
    pub added_km: f64,
    pub split_edges: usize,
}

/// Projection locale en mètres, assez juste à l'échelle d'un graphe partiel.
#[derive(Clone, Copy)]
struct Plane {
    lat0: f64,
    m_per_deg_lon: f64,
}

const M_PER_DEG_LAT: f64 = 111_195.0;

impl Plane {
    fn around(lat0: f64) -> Self {
        Self { lat0, m_per_deg_lon: M_PER_DEG_LAT * lat0.to_radians().cos() }
    }

    fn to_xy(self, c: Coordinate) -> (f64, f64) {
        (c.lon * self.m_per_deg_lon, (c.lat - self.lat0) * M_PER_DEG_LAT)
    }

    fn to_coord(self, (x, y): (f64, f64)) -> Coordinate {
        Coordinate { lat: y / M_PER_DEG_LAT + self.lat0, lon: x / self.m_per_deg_lon }
    }
}

/// Point de la projection et paramètre `t` le long de `a → b`.
fn project_on_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) };
    let (qx, qy) = (a.0 + t * dx, a.1 + t * dy);
    (((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt(), t)
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

const CELL_M: f64 = 50.0;

fn cell(p: (f64, f64)) -> (i64, i64) {
    ((p.0 / CELL_M).floor() as i64, (p.1 / CELL_M).floor() as i64)
}

/// Géométrie complète d'une arête OSM, extrémités comprises, en mètres.
struct OsmLine {
    points: Vec<(f64, f64)>,
}

/// Endroit d'une voie OSM : segment `seg` de l'arête `edge`, à la fraction `t`.
#[derive(Clone, Copy)]
struct OsmSpot {
    edge: usize,
    seg: usize,
    t: f64,
    dist_m: f64,
}

/// Index des segments OSM par case de `CELL_M`.
struct OsmIndex {
    lines: Vec<OsmLine>,
    cells: HashMap<(i64, i64), Vec<(u32, u32)>>,
}

impl OsmIndex {
    fn build(graph: &GraphFile, coords: &HashMap<u64, Coordinate>, plane: Plane) -> Self {
        let mut lines = Vec::with_capacity(graph.edges.len());
        let mut cells: HashMap<(i64, i64), Vec<(u32, u32)>> = HashMap::new();
        for (edge_idx, edge) in graph.edges.iter().enumerate() {
            let (Some(&from), Some(&to)) = (coords.get(&edge.from), coords.get(&edge.to)) else {
                lines.push(OsmLine { points: Vec::new() });
                continue;
            };
            let points: Vec<(f64, f64)> = std::iter::once(from)
                .chain(edge.waypoints.iter().copied())
                .chain(std::iter::once(to))
                .map(|c| plane.to_xy(c))
                .collect();
            for (seg, pair) in points.windows(2).enumerate() {
                let (lo, hi) = (cell(pair[0]), cell(pair[1]));
                for cx in lo.0.min(hi.0)..=lo.0.max(hi.0) {
                    for cy in lo.1.min(hi.1)..=lo.1.max(hi.1) {
                        cells.entry((cx, cy)).or_default().push((edge_idx as u32, seg as u32));
                    }
                }
            }
            lines.push(OsmLine { points });
        }
        Self { lines, cells }
    }

    /// Voie OSM la plus proche de `p` dans un rayon de `radius_m`.
    fn nearest(&self, p: (f64, f64), radius_m: f64) -> Option<OsmSpot> {
        let reach = (radius_m / CELL_M).ceil() as i64;
        let (cx, cy) = cell(p);
        let mut best: Option<OsmSpot> = None;
        for x in cx - reach..=cx + reach {
            for y in cy - reach..=cy + reach {
                let Some(entries) = self.cells.get(&(x, y)) else { continue };
                for &(edge, seg) in entries {
                    let points = &self.lines[edge as usize].points;
                    let (d, t) = project_on_segment(p, points[seg as usize], points[seg as usize + 1]);
                    if d <= radius_m && best.map_or(true, |b| d < b.dist_m) {
                        best = Some(OsmSpot { edge: edge as usize, seg: seg as usize, t, dist_m: d });
                    }
                }
            }
        }
        best
    }

    fn point_at(&self, spot: OsmSpot) -> (f64, f64) {
        let points = &self.lines[spot.edge].points;
        let (a, b) = (points[spot.seg], points[spot.seg + 1]);
        (a.0 + spot.t * (b.0 - a.0), a.1 + spot.t * (b.1 - a.1))
    }
}

/// Nœuds créés par la fusion, indexés pour être retrouvés au lieu d'être
/// créés deux fois au même endroit.
struct NewNodes {
    next_id: u64,
    cells: HashMap<(i64, i64), Vec<(u64, (f64, f64))>>,
    records: Vec<NodeRecord>,
}

impl NewNodes {
    fn find(&self, p: (f64, f64)) -> Option<u64> {
        let (cx, cy) = cell(p);
        (cx - 1..=cx + 1)
            .flat_map(|x| (cy - 1..=cy + 1).map(move |y| (x, y)))
            .filter_map(|c| self.cells.get(&c))
            .flatten()
            .filter(|(_, q)| dist(p, *q) <= SAME_NODE_M)
            .min_by(|a, b| dist(p, a.1).total_cmp(&dist(p, b.1)))
            .map(|(id, _)| *id)
    }

    fn create(&mut self, p: (f64, f64), plane: Plane) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.cells.entry(cell(p)).or_default().push((id, p));
        let c = plane.to_coord(p);
        self.records.push(NodeRecord { id, lat: c.lat, lon: c.lon, elevation: None, population_density: 0.0 });
        id
    }
}

/// Coupure demandée sur une voie OSM.
struct Split {
    seg: usize,
    t: f64,
    node: u64,
}

struct Merger<'a> {
    plane: Plane,
    osm: OsmIndex,
    graph: &'a GraphFile,
    nodes: NewNodes,
    splits: HashMap<usize, Vec<Split>>,
    new_edges: Vec<EdgeRecord>,
}

impl Merger<'_> {
    /// Nœud de raccordement sur la voie OSM `spot` : une extrémité de l'arête
    /// si elle est toute proche, sinon un nœud de coupure.
    fn junction_on(&mut self, spot: OsmSpot) -> (u64, (f64, f64)) {
        let at = self.osm.point_at(spot);
        let edge = &self.graph.edges[spot.edge];
        let points = &self.osm.lines[spot.edge].points;
        if dist(at, points[0]) <= SAME_NODE_M {
            return (edge.from, points[0]);
        }
        if dist(at, *points.last().unwrap()) <= SAME_NODE_M {
            return (edge.to, *points.last().unwrap());
        }
        if let Some(id) = self.nodes.find(at) {
            return (id, at);
        }
        let id = self.nodes.create(at, self.plane);
        self.splits.entry(spot.edge).or_default().push(Split { seg: spot.seg, t: spot.t, node: id });
        (id, at)
    }

    /// Nœud au bout d'un morceau IGN. `on_osm` : le bout est un point déjà
    /// couvert par OSM, à raccorder à coup sûr.
    fn end_node(&mut self, p: (f64, f64), on_osm: bool) -> (u64, (f64, f64)) {
        if !on_osm {
            // Bout d'un tronçon IGN : les tronçons voisins s'y rejoignent.
            if let Some(id) = self.nodes.find(p) {
                return (id, p);
            }
        }
        match self.osm.nearest(p, CONNECT_M) {
            Some(spot) => self.junction_on(spot),
            None => (self.nodes.create(p, self.plane), p),
        }
    }

    fn add_troncon(&mut self, troncon: &IgnTroncon) {
        let vertices: Vec<(f64, f64)> = troncon.line.iter().map(|&c| self.plane.to_xy(c)).collect();

        // Échantillons : (point, segment IGN, couvert par OSM ?).
        let mut samples: Vec<((f64, f64), usize, bool)> = Vec::new();
        for (seg, pair) in vertices.windows(2).enumerate() {
            let steps = ((dist(pair[0], pair[1]) / SAMPLE_M).ceil() as usize).max(1);
            for step in 0..steps {
                let t = step as f64 / steps as f64;
                let p = (pair[0].0 + t * (pair[1].0 - pair[0].0), pair[0].1 + t * (pair[1].1 - pair[0].1));
                samples.push((p, seg, false));
            }
        }
        let last = *vertices.last().unwrap();
        samples.push((last, vertices.len() - 1, false));
        for sample in samples.iter_mut() {
            sample.2 = self.osm.nearest(sample.0, COVERED_M).is_some();
        }

        let mut i = 0;
        while i < samples.len() {
            if samples[i].2 {
                i += 1;
                continue;
            }
            let first = i;
            while i < samples.len() && !samples[i].2 {
                i += 1;
            }
            let last_uncovered = i - 1;
            let gap_m = dist(samples[first].0, samples[last_uncovered].0) + SAMPLE_M;
            if gap_m < MIN_GAP_M {
                continue;
            }
            // Le morceau va du dernier point couvert avant l'écart au premier
            // après, quand ils existent : c'est par eux qu'il rejoint OSM.
            let lo = first.saturating_sub(1);
            let hi = (last_uncovered + 1).min(samples.len() - 1);
            self.add_piece(&samples, &vertices, lo, hi, troncon.nature);
        }
    }

    fn add_piece(
        &mut self,
        samples: &[((f64, f64), usize, bool)],
        vertices: &[(f64, f64)],
        lo: usize,
        hi: usize,
        nature: IgnNature,
    ) {
        let (start, start_seg, start_covered) = samples[lo];
        let (end, end_seg, end_covered) = samples[hi];

        let mut line = vec![start];
        for &v in &vertices[start_seg + 1..=end_seg.min(vertices.len() - 1)] {
            if dist(v, *line.last().unwrap()) > 0.5 {
                line.push(v);
            }
        }
        if dist(end, *line.last().unwrap()) > 0.5 {
            line.push(end);
        }

        let (from, from_at) = self.end_node(start, start_covered);
        let (to, to_at) = self.end_node(end, end_covered);
        if from == to {
            return;
        }
        // Le nœud de raccordement est sur la voie OSM, à quelques mètres du
        // tracé IGN : on l'y prolonge.
        line[0] = from_at;
        *line.last_mut().unwrap() = to_at;

        let coords: Vec<Coordinate> = line.iter().map(|&p| self.plane.to_coord(p)).collect();
        let length_m = coords.windows(2).map(|w| haversine_km(w[0], w[1]) * 1000.0).sum();
        self.new_edges.push(EdgeRecord {
            from,
            to,
            surface: nature.surface(),
            length_m,
            waypoints: coords[1..coords.len() - 1].to_vec(),
            forest_ratio: 0.0,
            habitation_pressure: 0.0,
            density_pressure: 0.0,
            ign: true,
        });
    }
}

/// Coupe l'arête OSM `edge` aux points demandés. La première part remplace
/// l'arête, les suivantes sont ajoutées à la fin de `out`.
fn split_edge(edge: &EdgeRecord, line: &[(f64, f64)], mut splits: Vec<Split>, plane: Plane) -> Vec<EdgeRecord> {
    splits.sort_by(|a, b| (a.seg, a.t).partial_cmp(&(b.seg, b.t)).unwrap());

    let mut parts = Vec::with_capacity(splits.len() + 1);
    let mut from = edge.from;
    let mut current: Vec<(f64, f64)> = vec![line[0]];
    let mut next_vertex = 1;
    let mut push_part = |from: u64, to: u64, points: &[(f64, f64)]| {
        let coords: Vec<Coordinate> = points.iter().map(|&p| plane.to_coord(p)).collect();
        parts.push(EdgeRecord {
            from,
            to,
            surface: edge.surface,
            length_m: coords.windows(2).map(|w| haversine_km(w[0], w[1]) * 1000.0).sum(),
            waypoints: coords[1..coords.len() - 1].to_vec(),
            forest_ratio: edge.forest_ratio,
            habitation_pressure: edge.habitation_pressure,
            density_pressure: edge.density_pressure,
            ign: edge.ign,
        });
    };

    for split in &splits {
        while next_vertex <= split.seg {
            current.push(line[next_vertex]);
            next_vertex += 1;
        }
        let (a, b) = (line[split.seg], line[split.seg + 1]);
        let at = (a.0 + split.t * (b.0 - a.0), a.1 + split.t * (b.1 - a.1));
        if split.node == from {
            continue;
        }
        current.push(at);
        push_part(from, split.node, &current);
        from = split.node;
        current = vec![at];
    }
    while next_vertex < line.len() {
        current.push(line[next_vertex]);
        next_vertex += 1;
    }
    push_part(from, edge.to, &current);
    parts
}

/// Ajoute au graphe les morceaux de `troncons` absents d'OSM.
///
/// Les nœuds et arêtes ajoutés le sont à la fin de `graph.nodes` et
/// `graph.edges` : l'appelant peut ainsi reporter l'occupation du sol sur eux
/// seuls. Une arête OSM coupée garde sa première part en place, avec ses
/// attributs ; ses autres parts sont ajoutées à la fin.
pub fn merge_into(graph: &mut GraphFile, troncons: &[IgnTroncon]) -> MergeStats {
    if graph.nodes.is_empty() || troncons.is_empty() {
        return MergeStats::default();
    }
    let lat0 = graph.nodes.iter().map(|n| n.lat).sum::<f64>() / graph.nodes.len() as f64;
    let plane = Plane::around(lat0);
    let coords: HashMap<u64, Coordinate> =
        graph.nodes.iter().map(|n| (n.id, Coordinate { lat: n.lat, lon: n.lon })).collect();
    let next_id = graph.nodes.iter().map(|n| n.id).max().unwrap_or(0).max(NODE_ID_BASE - 1) + 1;

    let mut merger = Merger {
        plane,
        osm: OsmIndex::build(graph, &coords, plane),
        graph,
        nodes: NewNodes { next_id, cells: HashMap::new(), records: Vec::new() },
        splits: HashMap::new(),
        new_edges: Vec::new(),
    };
    for troncon in troncons {
        merger.add_troncon(troncon);
    }

    let Merger { osm, nodes, splits, new_edges, .. } = merger;
    let stats = MergeStats {
        added_edges: new_edges.len(),
        added_km: new_edges.iter().map(|e| e.length_m).sum::<f64>() / 1000.0,
        split_edges: splits.len(),
    };

    let mut extra_parts = Vec::new();
    for (edge_idx, edge_splits) in splits {
        let mut parts = split_edge(&graph.edges[edge_idx], &osm.lines[edge_idx].points, edge_splits, plane);
        let first = parts.remove(0);
        graph.edges[edge_idx] = first;
        extra_parts.extend(parts);
    }

    graph.nodes.extend(nodes.records);
    graph.edges.extend(new_edges);
    graph.edges.extend(extra_parts);
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAT: f64 = 47.84;
    const LON: f64 = 4.74;

    /// Point à `x` m à l'est et `y` m au nord de l'origine des tests.
    fn at(x: f64, y: f64) -> Coordinate {
        let plane = Plane::around(LAT);
        plane.to_coord((LON * plane.m_per_deg_lon + x, y))
    }

    fn node(id: u64, c: Coordinate) -> NodeRecord {
        NodeRecord { id, lat: c.lat, lon: c.lon, elevation: None, population_density: 0.0 }
    }

    fn edge(from: u64, to: u64, length_m: f64) -> EdgeRecord {
        EdgeRecord {
            from,
            to,
            surface: SurfaceType::Dirt,
            length_m,
            waypoints: vec![],
            forest_ratio: 0.5,
            habitation_pressure: 0.0,
            density_pressure: 0.0,
            ign: false,
        }
    }

    fn chemin(points: &[(f64, f64)]) -> IgnTroncon {
        IgnTroncon { nature: IgnNature::Chemin, line: points.iter().map(|&(x, y)| at(x, y)).collect() }
    }

    /// Une voie OSM est-ouest de 1 km, le long de y = 0.
    fn osm_road() -> GraphFile {
        GraphFile {
            nodes: vec![node(1, at(0.0, 0.0)), node(2, at(1000.0, 0.0))],
            edges: vec![edge(1, 2, 1000.0)],
        }
    }

    #[test]
    fn a_path_already_in_osm_is_not_added_again() {
        let mut graph = osm_road();
        // Même voie, décalée de 8 m : l'écart habituel entre les deux sources.
        let stats = merge_into(&mut graph, &[chemin(&[(0.0, 8.0), (1000.0, 8.0)])]);

        assert_eq!(stats.added_edges, 0);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.nodes.len(), 2);
    }

    #[test]
    fn a_missing_path_is_joined_to_the_road_by_splitting_it() {
        let mut graph = osm_road();
        // Chemin qui part de la voie en son milieu et monte 500 m au nord.
        let stats = merge_into(&mut graph, &[chemin(&[(400.0, 0.0), (400.0, 500.0)])]);

        assert_eq!(stats.added_edges, 1);
        assert_eq!(stats.split_edges, 1);
        let ign: Vec<&EdgeRecord> = graph.edges.iter().filter(|e| e.ign).collect();
        assert_eq!(ign.len(), 1);
        assert!((ign[0].length_m - 500.0).abs() < 15.0, "longueur {}", ign[0].length_m);

        // La voie coupée en deux parts, qui gardent ses attributs et se
        // rejoignent sur le nœud de départ du chemin.
        let road: Vec<&EdgeRecord> = graph.edges.iter().filter(|e| !e.ign).collect();
        assert_eq!(road.len(), 2);
        assert!(road.iter().all(|e| e.forest_ratio == 0.5));
        let total: f64 = road.iter().map(|e| e.length_m).sum();
        assert!((total - 1000.0).abs() < 1.0, "longueur totale {total}");
        let junction = ign[0].from;
        assert!(road.iter().any(|e| e.to == junction) && road.iter().any(|e| e.from == junction));
    }

    #[test]
    fn a_path_leaving_the_road_starts_where_it_leaves_it() {
        let mut graph = osm_road();
        // Le tronçon IGN longe la voie sur 300 m, puis s'en écarte vers le nord.
        merge_into(&mut graph, &[chemin(&[(100.0, 5.0), (400.0, 5.0), (400.0, 400.0)])]);

        let ign: Vec<&EdgeRecord> = graph.edges.iter().filter(|e| e.ign).collect();
        assert_eq!(ign.len(), 1, "seule la partie hors de la voie est ajoutée");
        let start = graph.nodes.iter().find(|n| n.id == ign[0].from).unwrap();
        let plane = Plane::around(LAT);
        let (x, y) = plane.to_xy(Coordinate { lat: start.lat, lon: start.lon });
        let x = x - LON * plane.m_per_deg_lon;
        assert!(y.abs() < 1.0, "raccord sur la voie, pas à côté : y = {y}");
        assert!((x - 400.0).abs() < 25.0, "raccord au point de départ du chemin : x = {x}");
    }

    #[test]
    fn neighbouring_ign_paths_share_their_meeting_point() {
        let mut graph = osm_road();
        // Deux tronçons IGN loin de toute voie, bout à bout.
        merge_into(
            &mut graph,
            &[chemin(&[(0.0, 300.0), (500.0, 300.0)]), chemin(&[(500.0, 300.0), (1000.0, 300.0)])],
        );

        let ign: Vec<&EdgeRecord> = graph.edges.iter().filter(|e| e.ign).collect();
        assert_eq!(ign.len(), 2);
        assert_eq!(ign[0].to, ign[1].from);
    }

    #[test]
    fn two_paths_meeting_the_road_at_the_same_spot_share_one_split() {
        let mut graph = osm_road();
        merge_into(
            &mut graph,
            &[chemin(&[(400.0, 0.0), (400.0, 300.0)]), chemin(&[(401.0, 0.0), (401.0, -300.0)])],
        );

        assert_eq!(graph.edges.iter().filter(|e| !e.ign).count(), 2, "une seule coupure");
        let ign: Vec<&EdgeRecord> = graph.edges.iter().filter(|e| e.ign).collect();
        assert_eq!(ign[0].from, ign[1].from);
    }

    #[test]
    fn a_road_split_twice_keeps_its_order() {
        let mut graph = osm_road();
        merge_into(
            &mut graph,
            &[chemin(&[(700.0, 0.0), (700.0, 300.0)]), chemin(&[(200.0, 0.0), (200.0, 300.0)])],
        );

        let road: Vec<&EdgeRecord> = graph.edges.iter().filter(|e| !e.ign).collect();
        assert_eq!(road.len(), 3);
        let mut lengths: Vec<f64> = road.iter().map(|e| e.length_m.round()).collect();
        lengths.sort_by(f64::total_cmp);
        assert_eq!(lengths, vec![200.0, 300.0, 500.0]);
    }

    /// Interroge le vrai service : `cargo test -- --ignored`.
    #[test]
    #[ignore = "réseau"]
    fn the_service_returns_the_paths_of_champ_louis() {
        let dir = std::env::temp_dir().join(format!("ign-test-{}", std::process::id()));
        let bbox = BoundingBox { min_lat: 47.834, max_lat: 47.845, min_lon: 4.735, max_lon: 4.755 };
        let troncons = load_or_fetch(&dir, &bbox, "champ-louis", &AtomicBool::new(false)).unwrap();
        // Relu du cache au second appel, à l'identique.
        let cached = load_or_fetch(&dir, &bbox, "champ-louis", &AtomicBool::new(true)).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert!(troncons.len() > 50, "{} tronçons", troncons.len());
        assert!(troncons.iter().any(|t| t.nature == IgnNature::Sentier));
        assert_eq!(cached.len(), troncons.len());
        let first = troncons[0].line[0];
        assert!((47.8..47.9).contains(&first.lat) && (4.7..4.8).contains(&first.lon), "{first:?}");
    }

    #[test]
    fn natures_map_to_surfaces() {
        assert_eq!(IgnNature::from_label("Route empierrée").map(IgnNature::surface), Some(SurfaceType::Dirt));
        assert_eq!(IgnNature::from_label("Sentier").map(IgnNature::surface), Some(SurfaceType::Trail));
        assert_eq!(IgnNature::from_label("Route à 1 chaussée"), None);
    }
}
