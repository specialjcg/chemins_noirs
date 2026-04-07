//! Extract building footprints from OSM PBF files.
//! Returns building polygons as lists of coordinates within a bounding box.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use osmpbf::{Element, ElementReader};
use serde::Serialize;

use crate::models::Coordinate;

#[derive(Debug, Serialize)]
pub struct Building {
    pub polygon: Vec<Coordinate>,
    pub center: Coordinate,
}

/// Extract buildings from a PBF file within a bounding box.
/// Returns a list of building polygons (closed coordinate rings).
pub fn extract_buildings(
    pbf_path: &Path,
    min_lat: f64,
    max_lat: f64,
    min_lon: f64,
    max_lon: f64,
) -> Result<Vec<Building>, String> {
    let reader = ElementReader::from_path(pbf_path)
        .map_err(|e| format!("Failed to open PBF: {}", e))?;

    // Pass 1: collect nodes in bbox and building ways
    let (node_entries, building_ways): (Vec<(i64, (f64, f64))>, Vec<(i64, Vec<i64>)>) =
        reader.par_map_reduce(
            |element| match element {
                Element::Node(node) => {
                    let lat = node.lat();
                    let lon = node.lon();
                    if lat >= min_lat && lat <= max_lat && lon >= min_lon && lon <= max_lon {
                        (vec![(node.id(), (lat, lon))], Vec::new())
                    } else {
                        (Vec::new(), Vec::new())
                    }
                }
                Element::DenseNode(node) => {
                    let lat = node.lat();
                    let lon = node.lon();
                    if lat >= min_lat && lat <= max_lat && lon >= min_lon && lon <= max_lon {
                        (vec![(node.id(), (lat, lon))], Vec::new())
                    } else {
                        (Vec::new(), Vec::new())
                    }
                }
                Element::Way(way) => {
                    let is_building = way.tags().any(|(k, _v)| k == "building");
                    if is_building {
                        let refs: Vec<i64> = way.refs().collect();
                        (Vec::new(), vec![(way.id(), refs)])
                    } else {
                        (Vec::new(), Vec::new())
                    }
                }
                _ => (Vec::new(), Vec::new()),
            },
            || (Vec::new(), Vec::new()),
            |(mut n1, mut w1), (n2, w2)| {
                n1.extend(n2);
                w1.extend(w2);
                (n1, w1)
            },
        ).map_err(|e| format!("PBF parse error: {}", e))?;

    let nodes: HashMap<i64, (f64, f64)> = node_entries.into_iter().collect();
    let bbox_node_ids: HashSet<i64> = nodes.keys().copied().collect();

    // Filter: keep buildings that have at least one node in bbox
    let building_ways: Vec<(i64, Vec<i64>)> = building_ways
        .into_iter()
        .filter(|(_, refs)| refs.iter().any(|id| bbox_node_ids.contains(id)))
        .collect();

    // Collect missing nodes (outside bbox but part of building polygons)
    let all_refs: HashSet<i64> = building_ways.iter()
        .flat_map(|(_, refs)| refs.iter())
        .copied()
        .collect();
    let missing: HashSet<i64> = all_refs.difference(&bbox_node_ids).copied().collect();

    let mut all_nodes = nodes;

    if !missing.is_empty() {
        // Pass 2: fetch missing nodes
        let reader2 = ElementReader::from_path(pbf_path)
            .map_err(|e| format!("Failed to reopen PBF: {}", e))?;

        let extra: Vec<(i64, (f64, f64))> = reader2.par_map_reduce(
            |element| match element {
                Element::Node(node) if missing.contains(&node.id()) => {
                    vec![(node.id(), (node.lat(), node.lon()))]
                }
                Element::DenseNode(node) if missing.contains(&node.id()) => {
                    vec![(node.id(), (node.lat(), node.lon()))]
                }
                _ => Vec::new(),
            },
            Vec::new,
            |mut a, b| { a.extend(b); a },
        ).map_err(|e| format!("PBF pass 2 error: {}", e))?;

        all_nodes.extend(extra);
    }

    // Build building polygons
    let mut buildings = Vec::new();
    for (_way_id, refs) in &building_ways {
        let polygon: Vec<Coordinate> = refs.iter()
            .filter_map(|id| all_nodes.get(id).map(|(lat, lon)| Coordinate { lat: *lat, lon: *lon }))
            .collect();

        if polygon.len() >= 3 {
            let center = Coordinate {
                lat: polygon.iter().map(|c| c.lat).sum::<f64>() / polygon.len() as f64,
                lon: polygon.iter().map(|c| c.lon).sum::<f64>() / polygon.len() as f64,
            };
            buildings.push(Building { polygon, center });
        }
    }

    tracing::info!("Extracted {} buildings in bbox", buildings.len());
    Ok(buildings)
}
