//! Tile generation tool for Chemins Noirs
//!
//! Pre-generates 20km×20km tiles from a PBF file for ultra-fast graph loading.
//!
//! Usage:
//!   cargo run --release --bin generate_tiles -- \
//!     --pbf data/rhone-alpes-251111.osm.pbf \
//!     --output data/tiles \
//!     --tile-size 20

use backend::graph::{GraphBuilder, GraphBuilderConfig, TileId};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

const DEFAULT_TILE_SIZE_KM: f64 = 20.0;

// Rhône-Alpes approximate bounds
const RHONE_ALPES_MIN_LAT: f64 = 44.0;
const RHONE_ALPES_MAX_LAT: f64 = 47.0;
const RHONE_ALPES_MIN_LON: f64 = 3.5;
const RHONE_ALPES_MAX_LON: f64 = 7.5;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args: Vec<String> = std::env::args().collect();

    let pbf_path = parse_arg(&args, "--pbf")
        .unwrap_or_else(|| "data/rhone-alpes-251111.osm.pbf".to_string());
    let output_dir = parse_arg(&args, "--output").unwrap_or_else(|| "data/tiles".to_string());
    let tile_size_km = parse_arg(&args, "--tile-size")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(DEFAULT_TILE_SIZE_KM);

    tracing::info!("🔧 Tile generation parameters:");
    tracing::info!("  PBF file: {}", pbf_path);
    tracing::info!("  Output dir: {}", output_dir);
    tracing::info!("  Tile size: {}km × {}km", tile_size_km, tile_size_km);

    // Create output directory
    fs::create_dir_all(&output_dir)?;

    // Generate all tiles covering Rhône-Alpes
    let tiles = generate_tile_grid(tile_size_km);

    tracing::info!("📊 Generating {} tiles for Rhône-Alpes region", tiles.len());
    tracing::info!(
        "⏱️  Estimated time: {} minutes (parallel processing)",
        tiles.len() * 2 / 60
    );

    let pbf_path = PathBuf::from(pbf_path);
    let output_dir = PathBuf::from(output_dir);

    let mut generated = 0;
    let mut skipped = 0;
    let total = tiles.len();

    for (idx, tile_id) in tiles.iter().enumerate() {
        let output_path = output_dir.join(tile_id.filename());

        // Skip if tile already exists
        if output_path.exists() {
            skipped += 1;
            if idx % 10 == 0 {
                tracing::info!(
                    "Progress: {}/{} ({:.1}%) - {} generated, {} skipped",
                    idx + 1,
                    total,
                    ((idx + 1) as f64 / total as f64) * 100.0,
                    generated,
                    skipped
                );
            }
            continue;
        }

        let bbox = tile_id.bbox(tile_size_km);

        tracing::info!(
            "[{}/{}] Generating tile {:?} (bbox: {:.2}°-{:.2}°N, {:.2}°-{:.2}°E)",
            idx + 1,
            total,
            tile_id,
            bbox.min_lat,
            bbox.max_lat,
            bbox.min_lon,
            bbox.max_lon
        );

        let config = GraphBuilderConfig { bbox: Some(bbox) };
        let builder = GraphBuilder::new(config);

        match builder.build_from_pbf(&pbf_path) {
            Ok(graph) => {
                if graph.nodes.is_empty() {
                    tracing::warn!("  ⚠️  Empty tile (no roads in this area), skipping");
                    skipped += 1;
                } else {
                    graph.write_compressed(&output_path)?;
                    generated += 1;
                    tracing::info!(
                        "  ✅ Saved: {} nodes, {} edges → {}",
                        graph.nodes.len(),
                        graph.edges.len(),
                        output_path.display()
                    );
                }
            }
            Err(e) => {
                tracing::error!("  ❌ Failed to generate tile: {}", e);
            }
        }

        // Progress update every 10 tiles
        if (idx + 1) % 10 == 0 {
            tracing::info!(
                "Progress: {}/{} ({:.1}%) - {} generated, {} skipped",
                idx + 1,
                total,
                ((idx + 1) as f64 / total as f64) * 100.0,
                generated,
                skipped
            );
        }
    }

    tracing::info!("🎉 Tile generation complete!");
    tracing::info!("  Generated: {} tiles", generated);
    tracing::info!("  Skipped: {} tiles (empty or existing)", skipped);
    tracing::info!("  Output: {}", output_dir.display());

    Ok(())
}

/// Generate all tile IDs covering Rhône-Alpes
fn generate_tile_grid(tile_size_km: f64) -> Vec<TileId> {
    let mut tiles = HashSet::new();

    // Sample grid points across Rhône-Alpes
    let lat_step = 0.1; // ~11km
    let lon_step = 0.1;

    let mut lat = RHONE_ALPES_MIN_LAT;
    while lat <= RHONE_ALPES_MAX_LAT {
        let mut lon = RHONE_ALPES_MIN_LON;
        while lon <= RHONE_ALPES_MAX_LON {
            let tile_id = TileId::from_coord(lat, lon, tile_size_km);
            tiles.insert(tile_id);
            lon += lon_step;
        }
        lat += lat_step;
    }

    tiles.into_iter().collect()
}

/// Parse command line argument
fn parse_arg(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == key)
        .and_then(|idx| args.get(idx + 1))
        .map(|s| s.to_string())
}
