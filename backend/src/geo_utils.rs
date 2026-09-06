use crate::models::Coordinate;

pub const EARTH_RADIUS_KM: f64 = 6_371.0;
const EARTH_RADIUS_M: f64 = 6_371_000.0;

pub fn haversine_km(a: Coordinate, b: Coordinate) -> f64 {
    let lat1 = a.lat.to_radians();
    let lat2 = b.lat.to_radians();
    let dlat = (b.lat - a.lat).to_radians();
    let dlon = (b.lon - a.lon).to_radians();

    let sin_dlat = (dlat / 2.0).sin();
    let sin_dlon = (dlon / 2.0).sin();

    let h = sin_dlat * sin_dlat + lat1.cos() * lat2.cos() * sin_dlon * sin_dlon;
    2.0 * EARTH_RADIUS_KM * h.sqrt().asin()
}

pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (lat1, lon1, lat2, lon2) = (
        lat1.to_radians(),
        lon1.to_radians(),
        lat2.to_radians(),
        lon2.to_radians(),
    );
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS_M * c
}

/// Fast equirectangular distance approximation for A* heuristic.
/// ~3-5x faster than haversine (no sin/asin, just cos + sqrt).
/// Admissible for distances < 100km (never overestimates).
pub fn fast_distance_km(a: Coordinate, b: Coordinate) -> f64 {
    let dlat = (b.lat - a.lat).to_radians();
    let dlon = (b.lon - a.lon).to_radians();
    let cos_mid = ((a.lat + b.lat) / 2.0).to_radians().cos();
    let x = dlon * cos_mid;
    (dlat * dlat + x * x).sqrt() * EARTH_RADIUS_KM
}

pub fn approximate_distance_km(path: &[Coordinate]) -> f64 {
    path.windows(2).map(|w| haversine_km(w[0], w[1])).sum()
}

pub fn compute_bounds(path: &[Coordinate]) -> (f64, f64, f64, f64) {
    let mut min_lat = f64::MAX;
    let mut max_lat = f64::MIN;
    let mut min_lon = f64::MAX;
    let mut max_lon = f64::MIN;

    for coord in path {
        min_lat = min_lat.min(coord.lat);
        max_lat = max_lat.max(coord.lat);
        min_lon = min_lon.min(coord.lon);
        max_lon = max_lon.max(coord.lon);
    }

    (min_lat, max_lat, min_lon, max_lon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haversine_same_point() {
        let point = Coordinate {
            lat: 45.0,
            lon: 5.0,
        };
        assert_eq!(haversine_km(point, point), 0.0);
    }

    #[test]
    fn test_haversine_symmetry() {
        let a = Coordinate {
            lat: 45.0,
            lon: 5.0,
        };
        let b = Coordinate {
            lat: 46.0,
            lon: 6.0,
        };
        assert_eq!(haversine_km(a, b), haversine_km(b, a));
    }

    #[test]
    fn test_haversine_m_zero_distance() {
        let dist = haversine_m(45.0, 5.0, 45.0, 5.0);
        assert!(dist.abs() < 0.01);
    }

    #[test]
    fn test_haversine_m_symmetry() {
        let dist1 = haversine_m(45.0, 5.0, 46.0, 6.0);
        let dist2 = haversine_m(46.0, 6.0, 45.0, 5.0);
        assert!((dist1 - dist2).abs() < 0.01);
    }

    #[test]
    fn test_fast_distance_close_to_haversine() {
        let a = Coordinate { lat: 45.0, lon: 5.0 };
        let b = Coordinate { lat: 45.5, lon: 5.5 };
        let hav = haversine_km(a, b);
        let fast = fast_distance_km(a, b);
        // Should be within 0.5% for short distances
        assert!((fast - hav).abs() / hav < 0.005, "fast={fast}, haversine={hav}");
    }

    #[test]
    fn test_fast_distance_admissible() {
        // fast_distance must never overestimate (admissible heuristic)
        let pairs = [
            (Coordinate { lat: 45.0, lon: 5.0 }, Coordinate { lat: 45.1, lon: 5.1 }),
            (Coordinate { lat: 44.0, lon: 3.0 }, Coordinate { lat: 44.5, lon: 3.5 }),
            (Coordinate { lat: 46.0, lon: 6.0 }, Coordinate { lat: 46.0, lon: 6.5 }),
        ];
        for (a, b) in pairs {
            let hav = haversine_km(a, b);
            let fast = fast_distance_km(a, b);
            assert!(fast <= hav * 1.001, "fast_distance overestimated: fast={fast}, haversine={hav}");
        }
    }

    #[test]
    fn test_approximate_distance_empty() {
        assert_eq!(approximate_distance_km(&[]), 0.0);
    }

    #[test]
    fn test_approximate_distance_single_point() {
        let path = vec![Coordinate {
            lat: 45.0,
            lon: 5.0,
        }];
        assert_eq!(approximate_distance_km(&path), 0.0);
    }
}

/// Recouvrement en miroir entre la fin de `tail` et le début de `head`.
///
/// Renvoie combien de points retirer à la fin de `tail` et combien sauter au
/// début de `head` pour les coudre sans repli.
///
/// Deux segments calculés séparément se rejoignent sur un point commun. Quand
/// ce point d'accroche est au bout d'une branche du réseau, le premier segment
/// descend l'y chercher et le second remonte par le même chemin : chacun est
/// juste, mais mis bout à bout ils dessinent une antenne parcourue deux fois.
/// Seule la couture peut le voir, d'où cette fonction.
///
/// Les points répétés sont ignorés de part et d'autre : un itinéraire routé
/// commence souvent par deux points distants de quelques millimètres, assez
/// pour survivre au dédoublonnage et décaler la comparaison d'un cran.
///
/// Sans repli, le recouvrement se limite au point commun et le résultat est
/// `(0, 1)` — exactement le « sauter le premier point » habituel.
pub fn fold_overlap(tail: &[Coordinate], head: &[Coordinate]) -> (usize, usize) {
    /// Deux points à moins de ça sont le même endroit du réseau.
    const SAME_M: f64 = 5.0;

    fn same(a: Coordinate, b: Coordinate) -> bool {
        haversine_m(a.lat, a.lon, b.lat, b.lon) < SAME_M
    }

    /// Indices à parcourir, dans l'ordre donné, sans les répétitions.
    fn distinct(points: &[Coordinate], order: impl Iterator<Item = usize>) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for i in order {
            if out.last().is_none_or(|&last| !same(points[last], points[i])) {
                out.push(i);
            }
        }
        out
    }

    let back = distinct(tail, (0..tail.len()).rev());
    let front = distinct(head, 0..head.len());

    let mut k = 0;
    while k < back.len() && k < front.len() && same(tail[back[k]], head[front[k]]) {
        k += 1;
    }

    if k == 0 {
        return (0, 0);
    }

    // On garde toujours au moins un point de chaque côté : un recouvrement
    // total signifierait que l'un des segments disparaît.
    let drop_tail = (tail.len() - 1 - back[k - 1]).min(tail.len() - 1);
    let skip_head = (front[k - 1] + 1).min(head.len() - 1);

    (drop_tail, skip_head)
}

#[cfg(test)]
mod fold_tests {
    use super::*;

    fn c(lat: f64, lon: f64) -> Coordinate {
        Coordinate { lat, lon }
    }

    #[test]
    fn plain_junction_just_skips_the_shared_point() {
        let tail = vec![c(45.000, 5.000), c(45.001, 5.001)];
        let head = vec![c(45.001, 5.001), c(45.002, 5.002)];

        assert_eq!(fold_overlap(&tail, &head), (0, 1));
    }

    #[test]
    fn a_mirrored_tail_is_cut_back_to_the_branch_point() {
        // Le second segment remonte exactement le chemin du premier sur trois
        // points avant de repartir ailleurs.
        let tail = vec![c(45.000, 5.000), c(45.001, 5.000), c(45.002, 5.000), c(45.003, 5.000)];
        let head = vec![c(45.003, 5.000), c(45.002, 5.000), c(45.001, 5.000), c(45.001, 5.005)];

        let (drop_tail, skip_head) = fold_overlap(&tail, &head);
        assert_eq!((drop_tail, skip_head), (2, 3));

        let mut stitched = tail[..tail.len() - drop_tail].to_vec();
        stitched.extend_from_slice(&head[skip_head..]);

        let expected = [(45.000, 5.000), (45.001, 5.000), (45.001, 5.005)];
        assert_eq!(stitched.len(), expected.len(), "{stitched:?}");
        for (got, (lat, lon)) in stitched.iter().zip(expected) {
            assert!(
                (got.lat - lat).abs() < 1e-9 && (got.lon - lon).abs() < 1e-9,
                "attendu ({lat}, {lon}), obtenu ({}, {})",
                got.lat,
                got.lon
            );
        }
    }

    #[test]
    fn a_repeated_first_point_does_not_hide_the_fold() {
        // Le segment routé démarre par deux points à quelques millimètres l'un
        // de l'autre : sans dédoublonnage, la comparaison se décale et le
        // miroir passe inaperçu.
        let tail = vec![c(45.000, 5.000), c(45.001, 5.000), c(45.002, 5.000), c(45.003, 5.000)];
        let head = vec![
            c(45.003, 5.000),
            c(45.0030000001, 5.0000000001),
            c(45.002, 5.000),
            c(45.001, 5.000),
            c(45.001, 5.005),
        ];

        let (drop_tail, skip_head) = fold_overlap(&tail, &head);
        let mut stitched = tail[..tail.len() - drop_tail].to_vec();
        stitched.extend_from_slice(&head[skip_head..]);

        assert_eq!(stitched.len(), 3, "{stitched:?}");
        assert!((stitched[2].lon - 5.005).abs() < 1e-9, "{stitched:?}");
    }

    #[test]
    fn segments_that_do_not_meet_are_left_alone() {
        let tail = vec![c(45.000, 5.000), c(45.001, 5.000)];
        let head = vec![c(45.010, 5.000), c(45.011, 5.000)];

        assert_eq!(fold_overlap(&tail, &head), (0, 0));
    }

    #[test]
    fn a_fully_retraced_segment_keeps_one_point_on_each_side() {
        // Le second segment refait le premier à l'envers, en entier : on ne
        // doit pas vider le tracé.
        let tail = vec![c(45.000, 5.000), c(45.001, 5.000), c(45.002, 5.000)];
        let head = vec![c(45.002, 5.000), c(45.001, 5.000), c(45.000, 5.000)];

        let (drop_tail, skip_head) = fold_overlap(&tail, &head);
        assert!(drop_tail < tail.len(), "drop_tail={drop_tail}");
        assert!(skip_head < head.len(), "skip_head={skip_head}");
    }
}
