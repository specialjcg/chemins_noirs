use std::sync::Arc;

use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use backend::{
    AppState, create_router,
    engine::RouteEngine,
    models::{Coordinate, RouteRequest, RouteResponse},
};
use hyper::StatusCode;
use serde_json::json;
use tower::ServiceExt;

const SAMPLE_GRAPH: &str = include_str!("../data/sample_graph.json");

fn test_app() -> axum::Router {
    let engine = RouteEngine::from_reader(SAMPLE_GRAPH.as_bytes()).expect("graph");
    let state = AppState {
        engine: Arc::new(engine),
    };
    create_router(state)
}

#[tokio::test]
async fn route_endpoint_returns_gpx_payload() {
    let app = test_app();
    let payload = json!({
        "start": {"lat": 45.0005, "lon": 5.0005},
        "end": {"lat": 45.024, "lon": 5.034},
        "w_pop": 1.5,
        "w_paved": 4.0
    });

    let request = Request::builder()
        .method("POST")
        .uri("/api/route")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body: RouteResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(body.distance_km > 0.5);
    assert!(!body.gpx_base64.is_empty());
    assert!(body.path.len() >= 3);
}

#[tokio::test]
async fn route_respects_weights() {
    let app = test_app();

    let direct = RouteRequest {
        start: Coordinate {
            lat: 45.0005,
            lon: 5.0005,
        },
        end: Coordinate {
            lat: 45.024,
            lon: 5.034,
        },
        w_pop: 0.0,
        w_paved: 0.0,
    };
    let scenic = RouteRequest {
        w_pop: 0.0,
        w_paved: 5.0,
        ..direct
    };

    let make_request = |req: &RouteRequest| {
        Request::builder()
            .method("POST")
            .uri("/api/route")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(req).unwrap()))
            .unwrap()
    };

    let res_direct = app.clone().oneshot(make_request(&direct)).await.unwrap();
    let bytes_direct = to_bytes(res_direct.into_body(), 1024 * 1024).await.unwrap();
    let body_direct: RouteResponse = serde_json::from_slice(&bytes_direct).unwrap();

    let res_scenic = app.oneshot(make_request(&scenic)).await.unwrap();
    let bytes_scenic = to_bytes(res_scenic.into_body(), 1024 * 1024).await.unwrap();
    let body_scenic: RouteResponse = serde_json::from_slice(&bytes_scenic).unwrap();

    assert!(body_scenic.distance_km >= body_direct.distance_km);
}
