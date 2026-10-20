//! Densité de population : carreaux INSEE Filosofi 2021 de 200 m.
//!
//! La grille INSEE est alignée en ETRS89-LAEA (EPSG:3035). Un point GPS est
//! projeté dans ce système, puis rangé dans son carreau. Un carreau absent
//! n'a aucun habitant : c'est justement ce que cherche un chemin noir.
//! Le CSV vient de `scripts/build_population_grid.sh`.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

/// Côté d'un carreau, en mètres.
const CELL_M: f64 = 200.0;

/// Habitants par carreau au-delà desquels la pression vaut 1 : 40 habitants
/// sur 4 ha, soit 1000 hab/km², c'est déjà un bourg.
const SATURATION_INHABITANTS: f64 = 40.0;

/// Ellipsoïde GRS80 et paramètres de l'EPSG:3035.
const GRS80_A: f64 = 6_378_137.0;
const GRS80_INVERSE_F: f64 = 298.257_222_101;
const ORIGIN_LAT_DEG: f64 = 52.0;
const ORIGIN_LON_DEG: f64 = 10.0;
const FALSE_EASTING: f64 = 4_321_000.0;
const FALSE_NORTHING: f64 = 3_210_000.0;

/// Coordonnées ETRS89-LAEA (EPSG:3035) d'un point WGS84, en mètres : (est, nord).
pub fn wgs84_to_laea(lat: f64, lon: f64) -> (f64, f64) {
    // Formules de la note EPSG 7-2, Lambert azimutale équivalente sur l'ellipsoïde.
    let flattening = 1.0 / GRS80_INVERSE_F;
    let eccentricity_sq = flattening * (2.0 - flattening);
    let eccentricity = eccentricity_sq.sqrt();
    let authalic_q = |phi: f64| {
        let sin = phi.sin();
        (1.0 - eccentricity_sq)
            * (sin / (1.0 - eccentricity_sq * sin * sin)
                - ((1.0 - eccentricity * sin) / (1.0 + eccentricity * sin)).ln() / (2.0 * eccentricity))
    };

    let origin_lat = ORIGIN_LAT_DEG.to_radians();
    let q_pole = authalic_q(std::f64::consts::FRAC_PI_2);
    let beta_origin = (authalic_q(origin_lat) / q_pole).asin();
    let beta = (authalic_q(lat.to_radians()) / q_pole).asin();
    let authalic_radius = GRS80_A * (q_pole / 2.0).sqrt();
    let scale = GRS80_A * origin_lat.cos()
        / (1.0 - eccentricity_sq * origin_lat.sin().powi(2)).sqrt()
        / (authalic_radius * beta_origin.cos());

    let delta_lon = (lon - ORIGIN_LON_DEG).to_radians();
    let radial = authalic_radius
        * (2.0 / (1.0 + beta_origin.sin() * beta.sin() + beta_origin.cos() * beta.cos() * delta_lon.cos())).sqrt();

    let east = FALSE_EASTING + radial * scale * beta.cos() * delta_lon.sin();
    let north = FALSE_NORTHING
        + radial / scale * (beta_origin.cos() * beta.sin() - beta_origin.sin() * beta.cos() * delta_lon.cos());
    (east, north)
}

fn cell_of(east: f64, north: f64) -> (i32, i32) {
    ((east / CELL_M).floor() as i32, (north / CELL_M).floor() as i32)
}

/// Pression de population dans `0.0..=1.0`, sur une échelle logarithmique :
/// passer de 0 à 2 habitants compte bien plus que de 30 à 32.
pub fn density_pressure(inhabitants: u16) -> f64 {
    let pressure = (1.0 + f64::from(inhabitants)).ln() / (1.0 + SATURATION_INHABITANTS).ln();
    pressure.min(1.0)
}

/// Habitants par carreau, indexés par (colonne, ligne) dans la grille de 200 m.
pub struct PopulationGrid {
    cells: HashMap<(i32, i32), u16>,
}

impl PopulationGrid {
    /// Lit le CSV `east,north,ind` (coin sud-ouest du carreau en EPSG:3035).
    pub fn from_csv<R: BufRead>(reader: R) -> Result<Self, String> {
        let mut cells = HashMap::new();
        // Ligne 1 : l'en-tête.
        for (index, line) in reader.lines().enumerate().skip(1) {
            let line = line.map_err(|err| err.to_string())?;
            let line_number = index + 1;
            let (cell, inhabitants) =
                parse_cell(&line).ok_or_else(|| format!("ligne {line_number} illisible : {line}"))?;
            cells.insert(cell, inhabitants);
        }
        Ok(Self { cells })
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
        Self::from_csv(std::io::BufReader::new(file))
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn inhabitants_at(&self, lat: f64, lon: f64) -> u16 {
        let (east, north) = wgs84_to_laea(lat, lon);
        self.cells.get(&cell_of(east, north)).copied().unwrap_or(0)
    }

    pub fn pressure_at(&self, lat: f64, lon: f64) -> f64 {
        density_pressure(self.inhabitants_at(lat, lon))
    }
}

fn parse_cell(line: &str) -> Option<((i32, i32), u16)> {
    let mut fields = line.split(',');
    let east: f64 = fields.next()?.trim().parse().ok()?;
    let north: f64 = fields.next()?.trim().parse().ok()?;
    let inhabitants: u16 = fields.next()?.trim().parse().ok()?;
    Some((cell_of(east, north), inhabitants))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Références calculées par `cs2cs EPSG:4326 EPSG:3035` (GDAL 3.10).
    const REFERENCES: [(f64, f64, f64, f64); 3] = [
        (48.8566, 2.3522, 3_760_771.865, 2_889_484.802), // Paris
        (45.7640, 4.8357, 3_919_078.920, 2_530_754.338), // Lyon
        (47.2184, -1.5536, 3_449_245.653, 2_746_495.924), // Nantes
    ];

    #[test]
    fn la_projection_retrouve_les_references_de_gdal_au_centimetre() {
        for (lat, lon, east, north) in REFERENCES {
            let (got_east, got_north) = wgs84_to_laea(lat, lon);
            assert!(
                (got_east - east).abs() < 0.01 && (got_north - north).abs() < 0.01,
                "({lat}, {lon}) : attendu ({east}, {north}), obtenu ({got_east}, {got_north})"
            );
        }
    }

    #[test]
    fn un_point_tombe_dans_le_carreau_qui_le_contient() {
        // Paris : est 3 760 771 → carreau 3 760 600 ; nord 2 889 484 → 2 889 400.
        let csv = "east,north,ind\n3760600,2889400,120\n3760800,2889400,7\n";
        let grid = PopulationGrid::from_csv(csv.as_bytes()).expect("csv");

        assert_eq!(grid.len(), 2);
        assert_eq!(grid.inhabitants_at(48.8566, 2.3522), 120);
    }

    #[test]
    fn un_carreau_absent_n_a_aucun_habitant() {
        let grid = PopulationGrid::from_csv("east,north,ind\n3760600,2889400,120\n".as_bytes()).expect("csv");

        assert_eq!(grid.inhabitants_at(45.7640, 4.8357), 0);
        assert_eq!(grid.pressure_at(45.7640, 4.8357), 0.0);
    }

    #[test]
    fn une_ligne_illisible_est_signalee() {
        let error = PopulationGrid::from_csv("east,north,ind\n3760600,abc,1\n".as_bytes())
            .err()
            .expect("erreur attendue");
        assert!(error.contains("ligne 2"), "{error}");
    }

    #[test]
    fn la_pression_croit_de_zero_a_un_et_sature_a_mille_habitants_au_km2() {
        assert_eq!(density_pressure(0), 0.0);
        assert!(density_pressure(1) > 0.0);
        assert!(density_pressure(10) < density_pressure(20));
        assert!((density_pressure(40) - 1.0).abs() < 1e-12);
        assert_eq!(density_pressure(500), 1.0);
    }

    #[test]
    fn la_pression_est_logarithmique() {
        // Le premier hameau pèse plus que le dixième habitant de plus.
        let first = density_pressure(2) - density_pressure(0);
        let later = density_pressure(32) - density_pressure(30);
        assert!(first > 5.0 * later, "{first} vs {later}");
    }

    /// Contrôle sur la vraie grille : `cargo test --lib population -- --ignored --nocapture`.
    #[test]
    #[ignore = "lit data/population/insee_200m.csv"]
    fn la_vraie_grille_distingue_paris_de_la_lozere() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/population/insee_200m.csv");
        let started = std::time::Instant::now();
        let grid = PopulationGrid::load(&path).expect("grille");
        println!("{} carreaux lus en {:?}", grid.len(), started.elapsed());

        let paris = grid.inhabitants_at(48.8566, 2.3522);
        // Causse Méjean, loin de tout village.
        let causse = grid.inhabitants_at(44.2300, 3.4500);
        println!("Paris : {paris} hab, causse Méjean : {causse} hab");
        assert!(paris > 50, "Paris : {paris}");
        assert!(causse < 5, "causse : {causse}");
    }
}
