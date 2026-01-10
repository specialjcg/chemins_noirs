use backend::graph::{BoundingBox, GraphBuilder, GraphBuilderConfig};
use backend::models::Coordinate;
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::path::PathBuf;

fn benchmark_partial_graph_generation(c: &mut Criterion) {
    let pbf_path = PathBuf::from("data/rhone-alpes-251111.osm.pbf");

    // Skip if PBF file doesn't exist
    if !pbf_path.exists() {
        eprintln!("Skipping benchmark: PBF file not found at {:?}", pbf_path);
        return;
    }

    let mut group = c.benchmark_group("partial_graph_generation");

    // Test different route distances
    let test_cases = vec![
        ("2_waypoints_close",
         Coordinate { lat: 45.9306, lon: 4.5779 },
         Coordinate { lat: 45.9334, lon: 4.5783 },
         1.0),
        ("2_waypoints_medium",
         Coordinate { lat: 45.93, lon: 4.58 },
         Coordinate { lat: 45.95, lon: 4.60 },
         2.0),
        ("2_waypoints_far",
         Coordinate { lat: 45.90, lon: 4.55 },
         Coordinate { lat: 46.00, lon: 4.65 },
         5.0),
    ];

    for (name, start, end, margin_km) in test_cases {
        let bbox = BoundingBox::from_route(start, end, margin_km);

        group.bench_with_input(BenchmarkId::from_parameter(name), &bbox, |b, bbox| {
            b.iter(|| {
                let config = GraphBuilderConfig { bbox: Some(*bbox) };
                let builder = GraphBuilder::new(config);
                builder.build_from_pbf(black_box(&pbf_path))
            });
        });
    }

    group.finish();
}

fn benchmark_cache_performance(c: &mut Criterion) {
    let pbf_path = PathBuf::from("data/rhone-alpes-251111.osm.pbf");
    let cache_dir = PathBuf::from("data/cache");

    if !pbf_path.exists() {
        eprintln!("Skipping cache benchmark: PBF file not found");
        return;
    }

    let start = Coordinate { lat: 45.9306, lon: 4.5779 };
    let end = Coordinate { lat: 45.9334, lon: 4.5783 };
    let margin_km = 1.0;

    // Pre-generate cache
    let _ = GraphBuilder::build_partial_cached(&pbf_path, &cache_dir, start, end, margin_km);

    c.bench_function("cached_graph_load", |b| {
        b.iter(|| {
            GraphBuilder::build_partial_cached(
                black_box(&pbf_path),
                black_box(&cache_dir),
                black_box(start),
                black_box(end),
                black_box(margin_km),
            )
        });
    });
}

fn benchmark_closest_node(c: &mut Criterion) {
    use backend::engine::RouteEngine;
    

    let graph_path = PathBuf::from("data/cache");

    // Try to find any cached graph file
    let graph_files: Vec<PathBuf> = std::fs::read_dir(&graph_path)
        .ok().map(|entries| entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
                .map(|e| e.path())
                .collect())
        .unwrap_or_default();

    if graph_files.is_empty() {
        eprintln!("Skipping closest_node benchmark: No cached graphs found");
        return;
    }

    let engine = RouteEngine::from_file(&graph_files[0]).expect("Failed to load graph");

    let test_coords = [Coordinate { lat: 45.9306, lon: 4.5779 },
        Coordinate { lat: 45.9334, lon: 4.5783 },
        Coordinate { lat: 45.9350, lon: 4.5790 }];

    let mut group = c.benchmark_group("closest_node_kdtree");

    for (i, coord) in test_coords.iter().enumerate() {
        group.bench_with_input(BenchmarkId::from_parameter(i), coord, |b, coord| {
            b.iter(|| engine.closest_node(black_box(*coord)));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_partial_graph_generation,
    benchmark_cache_performance,
    benchmark_closest_node
);
criterion_main!(benches);
