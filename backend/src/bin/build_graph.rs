use std::path::PathBuf;

use backend::graph::{BoundingBox, GraphBuilder, GraphBuilderConfig};
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Build routing graph JSON from an OSM PBF slice"
)]
struct Args {
    /// Path to the OSM .pbf file (e.g. france-latest.osm.pbf or a regional extract)
    #[arg(long)]
    pbf: PathBuf,

    /// Output path where the JSON graph should be written
    #[arg(long)]
    output: PathBuf,

    /// Minimum latitude of the bounding box filter
    #[arg(long)]
    min_lat: Option<f64>,
    #[arg(long)]
    max_lat: Option<f64>,
    #[arg(long)]
    min_lon: Option<f64>,
    #[arg(long)]
    max_lon: Option<f64>,
}

impl Args {
    fn bbox(&self) -> Option<BoundingBox> {
        match (self.min_lat, self.max_lat, self.min_lon, self.max_lon) {
            (Some(min_lat), Some(max_lat), Some(min_lon), Some(max_lon)) => Some(BoundingBox {
                min_lat,
                max_lat,
                min_lon,
                max_lon,
            }),
            _ => None,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();
    tracing::info!("building graph from {:?} into {:?}", args.pbf, args.output);

    let builder = GraphBuilder::new(GraphBuilderConfig { bbox: args.bbox() });
    let graph = builder.build_from_pbf(&args.pbf)?;
    tracing::info!(
        "graph nodes={} edges={}",
        graph.nodes.len(),
        graph.edges.len()
    );
    graph.write_to_path(&args.output)?;
    tracing::info!("graph written to {:?}", args.output);

    Ok(())
}
