use osmpbf::{Element, ElementReader};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::graph::BoundingBox;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Poi {
    pub lat: f64,
    pub lon: f64,
    pub poi_type: String,
    pub name: Option<String>,
}

/// Extract POIs from a PBF file within a bounding box.
/// Looks for: drinking_water, shelter, alpine_hut, peak, saddle, parking, viewpoint
pub fn extract_pois_from_pbf(pbf_path: &Path, bbox: BoundingBox) -> Result<Vec<Poi>, String> {
    let reader =
        ElementReader::from_path(pbf_path).map_err(|e| format!("Failed to open PBF: {}", e))?;

    let pois = reader
        .par_map_reduce(
            |element| -> Vec<Poi> {
                let (lat, lon, tags) = match &element {
                    Element::Node(node) => {
                        let tags: Vec<(&str, &str)> = node.tags().collect();
                        (node.lat(), node.lon(), tags)
                    }
                    Element::DenseNode(node) => {
                        let tags: Vec<(&str, &str)> = node.tags().collect();
                        (node.lat(), node.lon(), tags)
                    }
                    _ => return Vec::new(),
                };

                // Check bbox
                if lat < bbox.min_lat
                    || lat > bbox.max_lat
                    || lon < bbox.min_lon
                    || lon > bbox.max_lon
                {
                    return Vec::new();
                }

                let name = tags
                    .iter()
                    .find(|(k, _)| *k == "name")
                    .map(|(_, v)| v.to_string());

                // Check for POI tags
                for (key, value) in &tags {
                    let poi_type = match (*key, *value) {
                        ("amenity", "drinking_water") => Some("water"),
                        ("amenity", "shelter") => Some("shelter"),
                        ("amenity", "parking") => Some("parking"),
                        ("tourism", "alpine_hut") => Some("hut"),
                        ("tourism", "viewpoint") => Some("viewpoint"),
                        ("natural", "peak") => Some("peak"),
                        ("natural", "saddle") => Some("saddle"),
                        ("natural", "spring") => Some("water"),
                        _ => None,
                    };

                    if let Some(pt) = poi_type {
                        return vec![Poi {
                            lat,
                            lon,
                            poi_type: pt.to_string(),
                            name,
                        }];
                    }
                }

                Vec::new()
            },
            Vec::new,
            |mut acc, pois| {
                acc.extend(pois);
                acc
            },
        )
        .map_err(|e| format!("PBF read error: {}", e))?;

    Ok(pois)
}
