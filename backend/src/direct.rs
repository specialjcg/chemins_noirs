//! Tronçons tirés en ligne droite, et ce qu'ils traversent.
//!
//! Un tronçon direct ne suit aucun chemin : le marcheur coupe à travers. On
//! le trace point par point pour que l'altitude s'y lise comme ailleurs, puis
//! on signale ce qui barre la route — maisons, terrain découvert, routes
//! goudronnées, pentes raides. Le tracé ne contourne rien : c'est au marcheur
//! de décider.

use crate::{
    geo_utils::haversine_m,
    landcover::LandCoverGrid,
    models::{Coordinate, ObstacleKind},
};

/// Espacement des points d'un tronçon direct. Assez fin pour qu'une maison
/// (une cellule de la grille d'occupation du sol) ne passe pas entre deux.
pub const STEP_M: f64 = 25.0;

/// Au-delà, on ne monte plus sans les mains.
pub const STEEP_SLOPE_PCT: f64 = 30.0;

/// Distance sur laquelle la pente est mesurée : d'un point au suivant, le
/// bruit du modèle de terrain ferait passer un talus pour une falaise.
const SLOPE_WINDOW_M: f64 = 50.0;

/// Obstacle repéré sur une polyligne, bornes comprises, en index de points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub kind: ObstacleKind,
    pub from: usize,
    pub to: usize,
}

/// Points régulièrement espacés de `a` à `b`, les deux inclus.
///
/// Interpolation en degrés : sur les quelques dizaines de kilomètres d'un
/// tronçon, l'écart avec le grand cercle reste sous le mètre.
pub fn straight_line(a: Coordinate, b: Coordinate, step_m: f64) -> Vec<Coordinate> {
    let steps = ((haversine_m(a.lat, a.lon, b.lat, b.lon) / step_m).ceil() as usize).max(1);
    (0..=steps)
        .map(|k| {
            let t = k as f64 / steps as f64;
            Coordinate {
                lat: a.lat + (b.lat - a.lat) * t,
                lon: a.lon + (b.lon - a.lon) * t,
            }
        })
        .collect()
}

/// Passages bâtis et à découvert, lus dans la grille d'occupation du sol.
pub fn landcover_spans(line: &[Coordinate], grid: &LandCoverGrid) -> Vec<Span> {
    let mut spans = runs(line.len(), ObstacleKind::Building, |i| {
        grid.is_built(line[i].lat, line[i].lon)
    });
    spans.extend(runs(line.len(), ObstacleKind::OpenGround, |i| {
        !grid.is_forest(line[i].lat, line[i].lon)
    }));
    spans
}

/// Passages plus raides que `STEEP_SLOPE_PCT`. `elevations` suit `line`
/// point pour point ; un point sans altitude ne compte pas comme raide.
pub fn steep_spans(line: &[Coordinate], elevations: &[Option<f64>]) -> Vec<Span> {
    let mut cum = Vec::with_capacity(line.len());
    let mut total = 0.0;
    for (i, c) in line.iter().enumerate() {
        if i > 0 {
            total += haversine_m(line[i - 1].lat, line[i - 1].lon, c.lat, c.lon);
        }
        cum.push(total);
    }

    // Pour chaque point, le premier point situé au moins une fenêtre plus loin.
    let window_end: Vec<Option<usize>> = (0..line.len())
        .map(|i| (i + 1..line.len()).find(|&j| cum[j] - cum[i] >= SLOPE_WINDOW_M))
        .collect();

    let steep = |i: usize| match (window_end[i], elevations.get(i).copied().flatten()) {
        (Some(j), Some(z0)) => match elevations.get(j).copied().flatten() {
            Some(z1) => (z1 - z0).abs() / (cum[j] - cum[i]) * 100.0 > STEEP_SLOPE_PCT,
            None => false,
        },
        _ => false,
    };

    // Une suite de points raides couvre aussi la fenêtre du dernier d'entre eux.
    runs(line.len(), ObstacleKind::SteepSlope, steep)
        .into_iter()
        .map(|span| Span {
            to: window_end[span.to].unwrap_or(span.to),
            ..span
        })
        .collect()
}

/// Suites maximales de points consécutifs où `hit` est vrai.
fn runs(len: usize, kind: ObstacleKind, hit: impl Fn(usize) -> bool) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut start = None;
    for i in 0..len {
        match (hit(i), start) {
            (true, None) => start = Some(i),
            (false, Some(from)) => {
                spans.push(Span { kind, from, to: i - 1 });
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        spans.push(Span { kind, from, to: len - 1 });
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::BoundingBox;

    fn c(lat: f64, lon: f64) -> Coordinate {
        Coordinate { lat, lon }
    }

    #[test]
    fn straight_line_is_evenly_spaced_and_keeps_both_ends() {
        let (a, b) = (c(47.0, 2.0), c(47.0, 2.01)); // ~760 m
        let line = straight_line(a, b, STEP_M);

        assert_eq!(line.first().unwrap().lon, a.lon);
        assert_eq!(line.last().unwrap().lon, b.lon);
        for pair in line.windows(2) {
            let d = haversine_m(pair[0].lat, pair[0].lon, pair[1].lat, pair[1].lon);
            assert!(d <= STEP_M && d > STEP_M * 0.9, "pas de {d} m");
        }
    }

    #[test]
    fn straight_line_between_identical_points_still_has_two_points() {
        assert_eq!(straight_line(c(47.0, 2.0), c(47.0, 2.0), STEP_M).len(), 2);
    }

    #[test]
    fn landcover_spans_find_the_house_and_the_clearing() {
        let bbox = BoundingBox {
            min_lat: 47.0,
            max_lat: 47.01,
            min_lon: 2.0,
            max_lon: 2.02,
        };
        let mut grid = LandCoverGrid::new(&bbox, 40.0);
        // Bois sur la première moitié, une maison au milieu de la seconde.
        grid.fill_polygon(
            &[(47.0, 2.0), (47.0, 2.01), (47.01, 2.01), (47.01, 2.0)],
            crate::landcover::Layer::Forest,
        );
        grid.fill_polygon(
            &[(47.0048, 2.0148), (47.0048, 2.0152), (47.0052, 2.0152), (47.0052, 2.0148)],
            crate::landcover::Layer::Built,
        );

        let line = straight_line(c(47.005, 2.001), c(47.005, 2.019), STEP_M);
        let spans = landcover_spans(&line, &grid);

        let built: Vec<_> = spans.iter().filter(|s| s.kind == ObstacleKind::Building).collect();
        assert_eq!(built.len(), 1, "une seule maison : {spans:?}");
        assert!((line[built[0].from].lon - 2.015).abs() < 0.001);

        let open: Vec<_> = spans.iter().filter(|s| s.kind == ObstacleKind::OpenGround).collect();
        assert_eq!(open.len(), 1, "une seule clairière : {spans:?}");
        assert!((line[open[0].from].lon - 2.01).abs() < 0.001);
        assert_eq!(open[0].to, line.len() - 1);
    }

    #[test]
    fn steep_spans_flag_the_cliff_and_not_the_gentle_slope() {
        let line = straight_line(c(47.0, 2.0), c(47.0, 2.01), STEP_M);
        // 5 % partout, sauf une marche de 40 m au milieu.
        let mid = line.len() / 2;
        let elevations: Vec<Option<f64>> = (0..line.len())
            .map(|i| Some(i as f64 * 1.25 + if i > mid { 40.0 } else { 0.0 }))
            .collect();

        let spans = steep_spans(&line, &elevations);

        assert_eq!(spans.len(), 1, "{spans:?}");
        assert!(spans[0].from <= mid && spans[0].to > mid, "{spans:?}");
    }

    #[test]
    fn missing_elevations_are_not_steep() {
        let line = straight_line(c(47.0, 2.0), c(47.0, 2.01), STEP_M);
        assert!(steep_spans(&line, &vec![None; line.len()]).is_empty());
    }
}
