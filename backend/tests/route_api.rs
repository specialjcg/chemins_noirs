use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use backend::{
    create_router,
    engine::RouteEngine,
    models::{Coordinate, RouteRequest, RouteResponse},
    AppState,
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

#[test]
fn regression_three_waypoint_itinerary() {
    let engine = RouteEngine::from_reader(SAMPLE_GRAPH.as_bytes()).expect("graph");

    // 3 waypoints: A (near node 1), B (near node 3), C (near node 6)
    let waypoint_a = Coordinate { lat: 45.0005, lon: 5.0005 };
    let waypoint_b = Coordinate { lat: 45.019, lon: 5.014 };
    let waypoint_c = Coordinate { lat: 45.024, lon: 5.034 };

    let w_pop = 1.0;
    let w_paved = 1.0;

    // Segment A → B
    let seg_ab = engine
        .find_path(&RouteRequest {
            start: waypoint_a,
            end: waypoint_b,
            w_pop,
            w_paved,
        })
        .expect("path A→B should exist");

    // Segment B → C
    let seg_bc = engine
        .find_path(&RouteRequest {
            start: waypoint_b,
            end: waypoint_c,
            w_pop,
            w_paved,
        })
        .expect("path B→C should exist");

    // Merge segments (skip first point of seg_bc to avoid duplicate)
    let mut full_path = seg_ab.clone();
    full_path.extend_from_slice(&seg_bc[1..]);

    let distance_km = backend::geo_utils::approximate_distance_km(&full_path);

    // --- Regression assertions (snapshot captured from current implementation) ---

    // Path structure
    assert_eq!(seg_ab.len(), 3, "segment A→B point count");
    assert_eq!(seg_bc.len(), 2, "segment B→C point count");
    assert_eq!(full_path.len(), 4, "merged path point count");

    // Total distance
    assert!(
        (distance_km - 4.2084657499).abs() < 0.0001,
        "distance_km regression: got {distance_km}"
    );

    // Exact path coordinates (nodes 1 → 2 → 3 → 6)
    let expected: [(f64, f64); 4] = [
        (45.000, 5.000),  // node 1
        (45.010, 5.005),  // node 2
        (45.020, 5.015),  // node 3
        (45.025, 5.035),  // node 6
    ];

    for (i, (coord, (elat, elon))) in full_path.iter().zip(expected.iter()).enumerate() {
        assert!(
            (coord.lat - elat).abs() < 1e-9 && (coord.lon - elon).abs() < 1e-9,
            "point[{i}] regression: got ({}, {}), expected ({elat}, {elon})",
            coord.lat,
            coord.lon
        );
    }
}
