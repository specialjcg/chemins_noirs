use std::{net::SocketAddr, sync::Arc};

use backend::{AppState, create_router, engine::RouteEngine};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const DEFAULT_PBF: &str = "data/rhone-alpes-251111.osm.pbf";
const DEFAULT_CACHE: &str = "data/cache";

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "backend=debug,axum::rejection=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Generate graph for Lyon metropolitan area (covers ~45.7-46.0, 4.5-4.9)
    let pbf = std::env::var("PBF_PATH").unwrap_or_else(|_| DEFAULT_PBF.to_string());
    let cache_path = format!("{}/lyon_area.json", std::env::var("CACHE_DIR").unwrap_or_else(|_| DEFAULT_CACHE.to_string()));

    let graph = if std::path::Path::new(&cache_path).exists() {
        tracing::info!("Loading cached graph from {cache_path}");
        backend::graph::GraphFile::read_from_path(&cache_path).expect("load cache")
    } else {
        tracing::info!("Generating Lyon area graph (45.7-46.0, 4.5-4.9) - first time only");
        let config = backend::graph::GraphBuilderConfig {
            bbox: Some(backend::graph::BoundingBox {
                min_lat: 45.7,
                max_lat: 46.0,
                min_lon: 4.5,
                max_lon: 4.9,
            }),
        };
        let builder = backend::graph::GraphBuilder::new(config);
        let g = builder.build_from_pbf(&pbf).expect("build graph");
        std::fs::create_dir_all(DEFAULT_CACHE).ok();
        g.write_to_path(&cache_path).ok();
        tracing::info!("Graph cached to {cache_path}");
        g
    };

    // Save to temp file and load (RouteEngine expects file path)
    let temp_path = format!("{}/temp_graph.json", std::env::var("CACHE_DIR").unwrap_or_else(|_| DEFAULT_CACHE.to_string()));
    graph.write_to_path(&temp_path).expect("write temp");
    let engine = RouteEngine::from_file(&temp_path).expect("create engine");
    tracing::info!("Engine ready");

    let state = AppState {
        engine: Arc::new(engine),
    };
    let app = create_router(state);

    let addr: SocketAddr = "0.0.0.0:8080".parse().expect("valid socket address");
    tracing::info!("starting backend on http://{addr}");
    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();
}
