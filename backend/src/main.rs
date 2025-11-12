use std::{net::SocketAddr, sync::Arc};

use backend::{AppState, create_router, engine::RouteEngine};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const SAMPLE_GRAPH_PATH: &str = "backend/data/sample_graph.json";

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "backend=debug,axum::rejection=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let graph_path = std::env::var("GRAPH_JSON").unwrap_or_else(|_| SAMPLE_GRAPH_PATH.to_string());
    let engine = RouteEngine::from_file(&graph_path).expect("load routing graph");
    tracing::info!("loaded routing graph from {graph_path}");

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
