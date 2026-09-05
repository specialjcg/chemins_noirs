//! Occupation du sol échantillonnée depuis un PBF OSM.
//!
//! Le moteur ne sait éviter que le goudron : rien ne lui dit s'il traverse un
//! bois ou un lotissement. Ce module rastérise deux couches OSM sur la bbox
//! d'un itinéraire — boisement et emprise bâtie — en grilles de bits que l'on
//! peut interroger en O(1) par coordonnée.
//!
//! On rastérise plutôt qu'on n'indexe les polygones : le graphe interroge la
//! couche des dizaines de milliers de fois par calcul, et une grille de 40 m
//! sur une bbox d'itinéraire tient dans quelques centaines de kilo-octets.

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, BufWriter, Write},
    path::Path,
};

use osmpbf::{Element, ElementReader};
use serde::{Deserialize, Serialize};

use crate::graph::BoundingBox;

/// Côté d'une cellule. 40 m distingue une allée forestière d'une lisière sans
/// faire exploser la grille : une bbox de 50 km tient en ~1,5 M cellules.
pub const DEFAULT_RESOLUTION_M: f64 = 40.0;

/// Marge autour de la bbox demandée, pour fermer les polygones qui la
/// débordent. Un bois tronqué se rastériserait de travers.
const POLYGON_MARGIN_DEG: f64 = 0.05;

/// Demi-côté du voisinage échantillonné pour la densité bâtie, en cellules.
/// 2 cellules à 40 m = un carré de 200 m, soit « le chemin longe le village »
/// plutôt que « le chemin est dans le village ».
const BUILT_NEIGHBOURHOOD: i64 = 2;

const M_PER_DEG_LAT: f64 = 111_320.0;

/// Couche rastérisée. Publique pour que l'on puisse fabriquer une grille sans
/// PBF — tests, et toute autre source d'occupation du sol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Forest,
    Built,
}

/// Grille binaire des deux couches sur une bbox.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LandCoverGrid {
    min_lat: f64,
    min_lon: f64,
    /// Hauteur d'une cellule en degrés de latitude.
    cell_lat: f64,
    /// Largeur d'une cellule en degrés de longitude, à la latitude médiane.
    cell_lon: f64,
    cols: usize,
    rows: usize,
    forest: Vec<u64>,
    built: Vec<u64>,
}

impl LandCoverGrid {
    pub fn new(bbox: &BoundingBox, resolution_m: f64) -> Self {
        let mid_lat = (bbox.min_lat + bbox.max_lat) / 2.0;
        let cell_lat = resolution_m / M_PER_DEG_LAT;
        // Un degré de longitude rétrécit avec la latitude ; sans ce cosinus les
        // cellules seraient deux fois trop larges dans le nord de la France.
        let cell_lon = resolution_m / (M_PER_DEG_LAT * mid_lat.to_radians().cos().abs().max(1e-6));

        let cols = (((bbox.max_lon - bbox.min_lon) / cell_lon).ceil() as usize).max(1);
        let rows = (((bbox.max_lat - bbox.min_lat) / cell_lat).ceil() as usize).max(1);
        let words = (cols * rows).div_ceil(64);

        Self {
            min_lat: bbox.min_lat,
            min_lon: bbox.min_lon,
            cell_lat,
            cell_lon,
            cols,
            rows,
            forest: vec![0; words],
            built: vec![0; words],
        }
    }

    fn cell_of(&self, lat: f64, lon: f64) -> Option<(usize, usize)> {
        let row = ((lat - self.min_lat) / self.cell_lat).floor();
        let col = ((lon - self.min_lon) / self.cell_lon).floor();

        if row < 0.0 || col < 0.0 {
            return None;
        }
        let (row, col) = (row as usize, col as usize);
        (row < self.rows && col < self.cols).then_some((row, col))
    }

    fn get(bits: &[u64], idx: usize) -> bool {
        bits[idx / 64] & (1u64 << (idx % 64)) != 0
    }

    fn set(bits: &mut [u64], idx: usize) {
        bits[idx / 64] |= 1u64 << (idx % 64);
    }

    /// Le point tombe-t-il dans un bois ? Hors grille répond `false`.
    pub fn is_forest(&self, lat: f64, lon: f64) -> bool {
        match self.cell_of(lat, lon) {
            Some((row, col)) => Self::get(&self.forest, row * self.cols + col),
            None => false,
        }
    }

    /// Fraction bâtie du voisinage du point, dans `0.0..=1.0`.
    ///
    /// C'est un dégradé et non un booléen : longer un lotissement doit coûter
    /// moins cher que le traverser, sinon le moteur n'a aucune raison de
    /// préférer la lisière.
    pub fn built_density(&self, lat: f64, lon: f64) -> f64 {
        let Some((row, col)) = self.cell_of(lat, lon) else {
            return 0.0;
        };

        let mut seen = 0.0;
        let mut built = 0.0;

        for d_row in -BUILT_NEIGHBOURHOOD..=BUILT_NEIGHBOURHOOD {
            for d_col in -BUILT_NEIGHBOURHOOD..=BUILT_NEIGHBOURHOOD {
                let r = row as i64 + d_row;
                let c = col as i64 + d_col;

                if r < 0 || c < 0 || r >= self.rows as i64 || c >= self.cols as i64 {
                    continue;
                }

                seen += 1.0;
                if Self::get(&self.built, r as usize * self.cols + c as usize) {
                    built += 1.0;
                }
            }
        }

        if seen == 0.0 {
            0.0
        } else {
            built / seen
        }
    }

    /// Remplit un polygone fermé, donné en `(lat, lon)`, par balayage de lignes.
    ///
    /// Règle pair-impair : sur chaque ligne de cellules on coupe les arêtes, on
    /// trie les abscisses et on remplit entre les paires. Les trous d'un
    /// multipolygone se creusent donc tout seuls si on les passe dans le même
    /// appel que le contour.
    pub fn fill_polygon(&mut self, ring: &[(f64, f64)], layer: Layer) {
        if ring.len() < 3 {
            return;
        }

        let min_lat = ring.iter().map(|(lat, _)| *lat).fold(f64::MAX, f64::min);
        let max_lat = ring.iter().map(|(lat, _)| *lat).fold(f64::MIN, f64::max);

        let first_row = (((min_lat - self.min_lat) / self.cell_lat).floor()).max(0.0) as usize;
        let last_row = {
            let r = ((max_lat - self.min_lat) / self.cell_lat).ceil();
            if r < 0.0 {
                return;
            }
            (r as usize).min(self.rows.saturating_sub(1))
        };

        let mut crossings: Vec<f64> = Vec::new();

        for row in first_row..=last_row {
            // Centre de la ligne : évite les égalités exactes sur un sommet,
            // qui compteraient l'arête deux fois.
            let y = self.min_lat + (row as f64 + 0.5) * self.cell_lat;

            crossings.clear();
            for window in 0..ring.len() {
                let (a_lat, a_lon) = ring[window];
                let (b_lat, b_lon) = ring[(window + 1) % ring.len()];

                if (a_lat > y) == (b_lat > y) {
                    continue;
                }

                let t = (y - a_lat) / (b_lat - a_lat);
                crossings.push(a_lon + t * (b_lon - a_lon));
            }

            if crossings.len() < 2 {
                continue;
            }
            crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            for pair in crossings.chunks_exact(2) {
                let from = ((pair[0] - self.min_lon) / self.cell_lon).floor();
                let to = ((pair[1] - self.min_lon) / self.cell_lon).ceil();

                if to < 0.0 || from >= self.cols as f64 {
                    continue;
                }

                let from = from.max(0.0) as usize;
                let to = (to as usize).min(self.cols.saturating_sub(1));

                for col in from..=to {
                    let idx = row * self.cols + col;
                    match layer {
                        Layer::Forest => Self::set(&mut self.forest, idx),
                        Layer::Built => Self::set(&mut self.built, idx),
                    }
                }
            }
        }
    }

    /// Nombre de cellules boisées — pour les tests et les mesures.
    pub fn forest_cells(&self) -> usize {
        self.forest.iter().map(|word| word.count_ones() as usize).sum()
    }

    /// Nombre de cellules bâties — pour les tests et les mesures.
    pub fn built_cells(&self) -> usize {
        self.built.iter().map(|word| word.count_ones() as usize).sum()
    }

    pub fn read_from_path(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let bytes = std::fs::read(path)?;
        postcard::from_bytes(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn write_to_path(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        let bytes = postcard::to_allocvec(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut writer = BufWriter::new(File::create(path)?);
        writer.write_all(&bytes)?;
        writer.flush()
    }
}

fn layer_of(tags: &[(String, String)]) -> Option<Layer> {
    let mut layer = None;

    for (key, value) in tags {
        match (key.as_str(), value.as_str()) {
            ("landuse", "forest") | ("natural", "wood") | ("landcover", "trees") => {
                return Some(Layer::Forest)
            }
            // L'emprise d'un village, pas ses bâtiments un par un : les
            // rastériser individuellement coûterait cher pour un résultat plus
            // troué.
            ("landuse", "residential")
            | ("landuse", "industrial")
            | ("landuse", "commercial")
            | ("landuse", "retail") => layer = Some(Layer::Built),
            _ => {}
        }
    }

    layer
}

/// Un anneau à rastériser, résolu ou en attente de ses nœuds.
struct PendingWay {
    refs: Vec<i64>,
    layer: Layer,
}

/// Construit la grille pour `bbox` en lisant `pbf_path`.
///
/// Deux passes sont nécessaires : les relations arrivent après les ways dans un
/// PBF, donc au moment où l'on croise un way on ne sait pas encore s'il est
/// membre d'un multipolygone boisé. La seconde passe ne se déclenche que si des
/// relations ont effectivement été trouvées.
pub fn build_from_pbf(
    pbf_path: impl AsRef<Path>,
    bbox: &BoundingBox,
    resolution_m: f64,
) -> Result<LandCoverGrid, osmpbf::Error> {
    let pbf_path = pbf_path.as_ref();

    // Les polygones débordent la bbox de l'itinéraire ; sans marge ils seraient
    // tronqués et le remplissage fuirait.
    let wide = BoundingBox {
        min_lat: bbox.min_lat - POLYGON_MARGIN_DEG,
        max_lat: bbox.max_lat + POLYGON_MARGIN_DEG,
        min_lon: bbox.min_lon - POLYGON_MARGIN_DEG,
        max_lon: bbox.max_lon + POLYGON_MARGIN_DEG,
    };

    let (nodes, tagged_ways, relation_members) = collect_first_pass(pbf_path, &wide)?;

    // Ways membres d'une relation boisée dont on n'a pas encore la géométrie.
    let missing: HashSet<i64> = relation_members
        .keys()
        .filter(|id| !tagged_ways.contains_key(id))
        .copied()
        .collect();

    let mut ways = tagged_ways;
    if !missing.is_empty() {
        for (id, refs) in collect_member_ways(pbf_path, &missing)? {
            if let Some(layer) = relation_members.get(&id) {
                ways.insert(id, PendingWay { refs, layer: *layer });
            }
        }
    }

    let mut grid = LandCoverGrid::new(&wide, resolution_m);
    let mut ring: Vec<(f64, f64)> = Vec::new();

    for way in ways.values() {
        ring.clear();
        ring.extend(way.refs.iter().filter_map(|id| nodes.get(id).copied()));

        // Un anneau amputé de la moitié de ses nœuds se remplirait n'importe
        // comment ; mieux vaut ne rien dessiner.
        if ring.len() < 3 || ring.len() * 2 < way.refs.len() {
            continue;
        }

        grid.fill_polygon(&ring, way.layer);
    }

    Ok(grid)
}

type FirstPass = (
    HashMap<i64, (f64, f64)>,
    HashMap<i64, PendingWay>,
    HashMap<i64, Layer>,
);

fn collect_first_pass(pbf_path: &Path, wide: &BoundingBox) -> Result<FirstPass, osmpbf::Error> {
    let reader = ElementReader::from_path(pbf_path)?;

    let (node_entries, way_entries, member_entries) = reader.par_map_reduce(
        |element| {
            let mut nodes: Vec<(i64, (f64, f64))> = Vec::new();
            let mut ways: Vec<(i64, PendingWay)> = Vec::new();
            let mut members: Vec<(i64, Layer)> = Vec::new();

            match element {
                Element::Node(node) => {
                    if wide.contains(shared::Coordinate { lat: node.lat(), lon: node.lon() }) {
                        nodes.push((node.id(), (node.lat(), node.lon())));
                    }
                }
                Element::DenseNode(node) => {
                    if wide.contains(shared::Coordinate { lat: node.lat(), lon: node.lon() }) {
                        nodes.push((node.id(), (node.lat(), node.lon())));
                    }
                }
                Element::Way(way) => {
                    let tags: Vec<(String, String)> = way
                        .tags()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect();

                    if let Some(layer) = layer_of(&tags) {
                        ways.push((
                            way.id(),
                            PendingWay { refs: way.refs().collect(), layer },
                        ));
                    }
                }
                Element::Relation(relation) => {
                    let tags: Vec<(String, String)> = relation
                        .tags()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect();

                    let is_multipolygon = tags
                        .iter()
                        .any(|(k, v)| k == "type" && (v == "multipolygon" || v == "boundary"));

                    if let (true, Some(layer)) = (is_multipolygon, layer_of(&tags)) {
                        for member in relation.members() {
                            // Les rôles `inner` passent dans le même sac : la
                            // règle pair-impair creuse les clairières d'elle-même.
                            if member.member_type == osmpbf::RelMemberType::Way {
                                members.push((member.member_id, layer));
                            }
                        }
                    }
                }
            }

            (nodes, ways, members)
        },
        || (Vec::new(), Vec::new(), Vec::new()),
        |(mut n1, mut w1, mut m1), (n2, w2, m2)| {
            n1.extend(n2);
            w1.extend(w2);
            m1.extend(m2);
            (n1, w1, m1)
        },
    )?;

    Ok((
        node_entries.into_iter().collect(),
        way_entries.into_iter().collect(),
        member_entries.into_iter().collect(),
    ))
}

fn collect_member_ways(
    pbf_path: &Path,
    wanted: &HashSet<i64>,
) -> Result<Vec<(i64, Vec<i64>)>, osmpbf::Error> {
    let reader = ElementReader::from_path(pbf_path)?;

    reader.par_map_reduce(
        |element| match element {
            Element::Way(way) if wanted.contains(&way.id()) => {
                vec![(way.id(), way.refs().collect::<Vec<i64>>())]
            }
            _ => Vec::new(),
        },
        Vec::new,
        |mut acc, batch| {
            acc.extend(batch);
            acc
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox() -> BoundingBox {
        BoundingBox { min_lat: 47.0, max_lat: 47.02, min_lon: 2.0, max_lon: 2.02 }
    }

    #[test]
    fn square_fills_its_inside_and_nothing_else() {
        let mut grid = LandCoverGrid::new(&bbox(), 40.0);
        grid.fill_polygon(
            &[(47.005, 2.005), (47.005, 2.015), (47.015, 2.015), (47.015, 2.005)],
            Layer::Forest,
        );

        assert!(grid.is_forest(47.010, 2.010), "le centre doit être boisé");
        assert!(!grid.is_forest(47.002, 2.010), "sous le carré : pas de bois");
        assert!(!grid.is_forest(47.010, 2.018), "à droite du carré : pas de bois");
    }

    #[test]
    fn inner_ring_carves_a_clearing() {
        let mut grid = LandCoverGrid::new(&bbox(), 40.0);
        // Contour et trou dans le même appel : la règle pair-impair doit
        // laisser le centre non boisé.
        grid.fill_polygon(
            &[
                (47.004, 2.004),
                (47.004, 2.016),
                (47.016, 2.016),
                (47.016, 2.004),
                (47.004, 2.004),
                (47.009, 2.009),
                (47.009, 2.011),
                (47.011, 2.011),
                (47.011, 2.009),
            ],
            Layer::Forest,
        );

        assert!(grid.is_forest(47.006, 2.010), "la lisière reste boisée");
        assert!(!grid.is_forest(47.010, 2.010), "la clairière doit être trouée");
    }

    #[test]
    fn outside_the_grid_is_never_forest() {
        let mut grid = LandCoverGrid::new(&bbox(), 40.0);
        grid.fill_polygon(
            &[(47.005, 2.005), (47.005, 2.015), (47.015, 2.015), (47.015, 2.005)],
            Layer::Forest,
        );

        assert!(!grid.is_forest(48.0, 2.010));
        assert!(!grid.is_forest(47.010, 3.0));
        assert_eq!(grid.built_density(48.0, 2.010), 0.0);
    }

    #[test]
    fn built_density_grades_the_edge_of_a_village() {
        let mut grid = LandCoverGrid::new(&bbox(), 40.0);
        // Bande bâtie sur la moitié gauche de la bbox.
        grid.fill_polygon(
            &[(47.000, 2.000), (47.000, 2.010), (47.020, 2.010), (47.020, 2.000)],
            Layer::Built,
        );

        let inside = grid.built_density(47.010, 2.005);
        let edge = grid.built_density(47.010, 2.010);
        let outside = grid.built_density(47.010, 2.018);

        assert!(inside > 0.9, "au cœur du bâti : {inside}");
        assert!(edge > 0.2 && edge < 0.9, "en lisière, un dégradé : {edge}");
        assert_eq!(outside, 0.0, "loin du bâti : {outside}");
    }

    #[test]
    fn layers_are_independent() {
        let mut grid = LandCoverGrid::new(&bbox(), 40.0);
        grid.fill_polygon(
            &[(47.005, 2.005), (47.005, 2.015), (47.015, 2.015), (47.015, 2.005)],
            Layer::Forest,
        );

        assert!(grid.forest_cells() > 0);
        assert_eq!(grid.built_cells(), 0);
        assert_eq!(grid.built_density(47.010, 2.010), 0.0);
    }

    #[test]
    fn tags_pick_the_right_layer() {
        let tag = |k: &str, v: &str| vec![(k.to_string(), v.to_string())];

        assert_eq!(layer_of(&tag("landuse", "forest")), Some(Layer::Forest));
        assert_eq!(layer_of(&tag("natural", "wood")), Some(Layer::Forest));
        assert_eq!(layer_of(&tag("landuse", "residential")), Some(Layer::Built));
        assert_eq!(layer_of(&tag("landuse", "farmland")), None);
        assert_eq!(layer_of(&tag("highway", "track")), None);
    }
}
