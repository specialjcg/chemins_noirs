//! Local index of OSM lodgings, extracted once from the routing PBF.
//!
//! Overpass is often saturated or down; the PBF already on disk holds the same
//! `tourism=*` objects. Extraction takes two passes over the file (tagged nodes
//! and ways, then the nodes of those ways for their centre) and the result is
//! cached next to the routing caches, keyed by the PBF path, size and mtime.
//! The data is as old as the PBF; relations (a handful of big campsites) are
//! not indexed.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use osmpbf::{Element, ElementReader};
use serde::{Deserialize, Serialize};

use crate::graph::BoundingBox;
use crate::lodgings::{lodging_from_tags, Lodging, TOURISM_KINDS};

/// Bump when the stored format or the extraction rules change.
const CACHE_VERSION: u32 = 1;

static INDEX: OnceLock<Arc<LodgingIndex>> = OnceLock::new();

/// The index, once loaded or built; None while it is still being prepared.
pub fn ready() -> Option<Arc<LodgingIndex>> {
    INDEX.get().cloned()
}

/// Loads the cached index or builds it from the PBF, then publishes it.
/// Blocking: call from a background thread at startup.
pub fn init(pbf_path: &Path, cache_dir: &Path) -> Result<usize, String> {
    let cache = cache_path(pbf_path, cache_dir)?;
    let index = match LodgingIndex::read(&cache) {
        Ok(index) => index,
        Err(_) => {
            let index = LodgingIndex::build_from_pbf(pbf_path)?;
            if let Err(e) = index.write(&cache) {
                tracing::warn!("lodging index: cache not written ({}): {e}", cache.display());
            }
            index
        }
    };
    let count = index.items.len();
    let _ = INDEX.set(Arc::new(index));
    Ok(count)
}

fn cache_path(pbf_path: &Path, cache_dir: &Path) -> Result<PathBuf, String> {
    let meta = std::fs::metadata(pbf_path)
        .map_err(|e| format!("PBF {} : {e}", pbf_path.display()))?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pbf_path.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok().hash(&mut hasher);
    CACHE_VERSION.hash(&mut hasher);
    Ok(cache_dir.join(format!("{:016x}.lodgings", hasher.finish())))
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LodgingIndex {
    items: Vec<Lodging>,
}

/// A tagged way waiting for the coordinates of its nodes.
struct PendingWay {
    lodging: Lodging,
    refs: Vec<i64>,
}

fn is_indexed_kind(kind: &str) -> bool {
    TOURISM_KINDS.contains(&kind)
}

fn lodging_of<'a>(
    osm_type: &str,
    id: i64,
    lat: f64,
    lon: f64,
    tags: impl Iterator<Item = (&'a str, &'a str)>,
) -> Option<Lodging> {
    let tags: HashMap<&str, &str> = tags.collect();
    if !tags.get("tourism").is_some_and(|k| is_indexed_kind(k)) {
        return None;
    }
    lodging_from_tags(osm_type, id, lat, lon, |k| tags.get(k).map(|v| v.to_string()))
}

/// Centre of the bounding box of the points, as Overpass `out center` does.
fn bbox_center(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let first = points.first()?;
    let (mut min, mut max) = (*first, *first);
    for &(lat, lon) in points {
        min = (min.0.min(lat), min.1.min(lon));
        max = (max.0.max(lat), max.1.max(lon));
    }
    Some(((min.0 + max.0) / 2.0, (min.1 + max.1) / 2.0))
}

impl LodgingIndex {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Lodgings inside the bbox whose kind is among `kinds`.
    pub fn query(&self, bbox: &BoundingBox, kinds: &[&str]) -> Vec<Lodging> {
        self.items
            .iter()
            .filter(|l| kinds.contains(&l.kind.as_str()))
            .filter(|l| bbox.contains(shared::Coordinate { lat: l.lat, lon: l.lon }))
            .cloned()
            .collect()
    }

    pub fn build_from_pbf(pbf_path: &Path) -> Result<Self, String> {
        let started = std::time::Instant::now();
        let open = || {
            ElementReader::from_path(pbf_path)
                .map_err(|e| format!("PBF {} : {e}", pbf_path.display()))
        };

        // Pass 1: tagged nodes are complete; tagged ways still need their nodes.
        let (mut items, ways) = open()?
            .par_map_reduce(
                |element| -> (Vec<Lodging>, Vec<PendingWay>) {
                    match element {
                        Element::Node(n) => {
                            (lodging_of("node", n.id(), n.lat(), n.lon(), n.tags()).into_iter().collect(), vec![])
                        }
                        Element::DenseNode(n) => {
                            (lodging_of("node", n.id(), n.lat(), n.lon(), n.tags()).into_iter().collect(), vec![])
                        }
                        Element::Way(w) => match lodging_of("way", w.id(), 0.0, 0.0, w.tags()) {
                            Some(lodging) => (vec![], vec![PendingWay { lodging, refs: w.refs().collect() }]),
                            None => (vec![], vec![]),
                        },
                        Element::Relation(_) => (vec![], vec![]),
                    }
                },
                || (Vec::new(), Vec::new()),
                |(mut n1, mut w1), (n2, w2)| {
                    n1.extend(n2);
                    w1.extend(w2);
                    (n1, w1)
                },
            )
            .map_err(|e| format!("PBF read error: {e}"))?;

        // Pass 2: coordinates of the nodes those ways reference, and only those.
        let wanted: HashSet<i64> = ways.iter().flat_map(|w| w.refs.iter().copied()).collect();
        let coords: HashMap<i64, (f64, f64)> = if wanted.is_empty() {
            HashMap::new()
        } else {
            open()?
                .par_map_reduce(
                    |element| match element {
                        Element::Node(n) if wanted.contains(&n.id()) => vec![(n.id(), (n.lat(), n.lon()))],
                        Element::DenseNode(n) if wanted.contains(&n.id()) => vec![(n.id(), (n.lat(), n.lon()))],
                        _ => vec![],
                    },
                    Vec::new,
                    |mut a, b| {
                        a.extend(b);
                        a
                    },
                )
                .map_err(|e| format!("PBF read error: {e}"))?
                .into_iter()
                .collect()
        };

        let way_count = ways.len();
        items.extend(ways.into_iter().filter_map(|w| {
            let points: Vec<(f64, f64)> = w.refs.iter().filter_map(|id| coords.get(id).copied()).collect();
            let (lat, lon) = bbox_center(&points)?;
            Some(Lodging { lat, lon, ..w.lodging })
        }));

        tracing::info!(
            "lodging index: {} lodgings ({} ways) from {} in {:.0}s",
            items.len(),
            way_count,
            pbf_path.display(),
            started.elapsed().as_secs_f64()
        );
        Ok(Self { items })
    }

    fn read(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        postcard::from_bytes(&bytes).map_err(|e| e.to_string())
    }

    fn write(&self, path: &Path) -> Result<(), String> {
        let bytes = postcard::to_allocvec(self).map_err(|e| e.to_string())?;
        // Written aside then renamed: a crash mid-write must not leave a
        // truncated cache that would be read as valid next time.
        let tmp = path.with_extension("lodgings.tmp");
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lodging(kind: &str, lat: f64, lon: f64) -> Lodging {
        lodging_from_tags("node", 1, lat, lon, |k| (k == "tourism").then(|| kind.to_string())).unwrap()
    }

    #[test]
    fn keeps_only_indexed_tourism_kinds() {
        let tags = [("tourism", "hotel"), ("name", "Le Relais")];
        let hotel = lodging_of("node", 7, 45.0, 5.0, tags.into_iter()).unwrap();
        assert_eq!((hotel.kind.as_str(), hotel.name.as_str()), ("hotel", "Le Relais"));

        assert!(lodging_of("node", 8, 45.0, 5.0, [("tourism", "viewpoint")].into_iter()).is_none());
        assert!(lodging_of("node", 9, 45.0, 5.0, [("amenity", "bench")].into_iter()).is_none());
    }

    #[test]
    fn way_centre_is_bbox_centre() {
        assert_eq!(bbox_center(&[(45.0, 5.0), (45.2, 5.4), (45.1, 5.1)]), Some((45.1, 5.2)));
        assert_eq!(bbox_center(&[]), None);
    }

    #[test]
    fn query_filters_by_bbox_and_kind() {
        let index = LodgingIndex {
            items: vec![lodging("hotel", 45.0, 5.0), lodging("camp_site", 45.0, 5.0), lodging("hotel", 47.0, 5.0)],
        };
        let bbox = BoundingBox { min_lat: 44.9, max_lat: 45.1, min_lon: 4.9, max_lon: 5.1 };
        let found = index.query(&bbox, &["hotel"]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "hotel");
    }

    #[test]
    fn cache_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.lodgings");
        let index = LodgingIndex { items: vec![lodging("hostel", 45.5, 4.5)] };
        index.write(&path).unwrap();
        let back = LodgingIndex::read(&path).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back.items[0].kind, "hostel");
        assert_eq!(back.items[0].lat, 45.5);
    }
}
