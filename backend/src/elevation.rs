use std::error::Error;
use std::path::PathBuf;
use std::sync::OnceLock;

use shared::{Coordinate, ElevationProfile};

use crate::dem::ArcAsciiDem;

/// Get elevation data for a batch of coordinates from local DEM
pub async fn get_elevations(coords: Vec<(f64, f64)>) -> Result<Vec<f64>, Box<dyn Error>> {
    if coords.is_empty() {
        return Ok(Vec::new());
    }

    let grid = local_dem_grid()
        .ok_or("Local DEM not available. Please ensure LOCAL_DEM_PATH is set or backend/data/dem/region.asc exists.")?;

    let mut values = Vec::with_capacity(coords.len());
    let mut missing_coords = Vec::new();

    for &(lat, lon) in &coords {
        match grid.sample(lat, lon) {
            Some(val) => values.push(val),
            None => {
                missing_coords.push((lat, lon));
            }
        }
    }

    if !missing_coords.is_empty() {
        tracing::warn!(
            "Local DEM does not cover {} coordinate(s): {:?}",
            missing_coords.len(),
            &missing_coords[..missing_coords.len().min(5)]
        );
        return Err(format!(
            "DEM coverage incomplete: {} coordinates outside DEM bounds",
            missing_coords.len()
        )
        .into());
    }

    tracing::debug!("Fetched {} elevations from local DEM", values.len());
    Ok(values)
}

fn local_dem_path() -> Option<PathBuf> {
    let path = std::env::var("LOCAL_DEM_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("backend/data/dem/region.asc"));
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn local_dem_grid() -> Option<&'static ArcAsciiDem> {
    static CACHE: OnceLock<Option<ArcAsciiDem>> = OnceLock::new();

    CACHE
        .get_or_init(|| {
            let path = local_dem_path()?;
            match ArcAsciiDem::from_path(&path) {
                Ok(grid) => {
                    tracing::info!("Loaded local DEM grid from {}", path.display());
                    Some(grid)
                }
                Err(err) => {
                    tracing::error!(
                        "Failed to load local DEM from {}: {}",
                        path.display(),
                        err
                    );
                    None
                }
            }
        })
        .as_ref()
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(values[values.len() / 2])
}

fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let (lat1, lon1, lat2, lon2) = (
        lat1.to_radians(),
        lon1.to_radians(),
        lat2.to_radians(),
        lon2.to_radians(),
    );
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    R * c
}

/// Smooth elevations by applying a small median filter and distance-aware clamping to reduce outliers.
fn smooth_elevation_profile(path: &[Coordinate], raw: &[Option<f64>]) -> Vec<Option<f64>> {
    let mut smoothed = Vec::with_capacity(raw.len());

    for (idx, value) in raw.iter().enumerate() {
        // Median over a 3-point window (prev, current, next) ignoring Nones
        let mut window: Vec<f64> = [-1i32, 0, 1]
            .iter()
            .filter_map(|offset| {
                let pos = idx as isize + *offset as isize;
                if pos >= 0 && (pos as usize) < raw.len() {
                    raw[pos as usize]
                } else {
                    None
                }
            })
            .collect();

        let median_val = median(&mut window);
        let mut candidate = median_val.or(*value);

        if let (Some(prev), Some(current)) = (smoothed.last().copied().flatten(), candidate) {
            // Distance-aware clamping: allow small vertical change for close points,
            // a bit more when points are spaced out.
            let dist_m = if idx > 0 {
                let a = &path[idx - 1];
                let b = &path[idx];
                haversine_m(a.lat, a.lon, b.lat, b.lon)
            } else {
                0.0
            };
            let max_delta = (dist_m * 0.2).clamp(8.0, 30.0); // meters
            candidate = Some(current.clamp(prev - max_delta, prev + max_delta));
        }

        smoothed.push(candidate);
    }

    smoothed
}

/// Create an elevation profile for a route path using local DEM
pub async fn create_elevation_profile(
    path: &[Coordinate],
) -> Result<ElevationProfile, Box<dyn Error>> {
    if path.is_empty() {
        return Ok(ElevationProfile {
            elevations: Vec::new(),
            min_elevation: None,
            max_elevation: None,
            total_ascent: 0.0,
            total_descent: 0.0,
        });
    }

    // Convert path to (lat, lon) tuples
    let coords: Vec<(f64, f64)> = path.iter().map(|c| (c.lat, c.lon)).collect();

    // Get elevations from local DEM
    let elevations_vec = get_elevations(coords).await?;
    let raw_elevations: Vec<Option<f64>> = elevations_vec.into_iter().map(Some).collect();
    let elevations = smooth_elevation_profile(path, &raw_elevations);

    // Calculate statistics
    let valid_elevations: Vec<f64> = elevations.iter().filter_map(|&e| e).collect();

    let min_elevation = valid_elevations
        .iter()
        .cloned()
        .fold(f64::INFINITY, f64::min);
    let max_elevation = valid_elevations
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);

    let min_elevation = if min_elevation.is_finite() {
        Some(min_elevation)
    } else {
        None
    };
    let max_elevation = if max_elevation.is_finite() {
        Some(max_elevation)
    } else {
        None
    };

    // Calculate total ascent and descent
    let mut total_ascent = 0.0;
    let mut total_descent = 0.0;

    for window in elevations.windows(2) {
        if let (Some(prev), Some(curr)) = (window[0], window[1]) {
            let diff = curr - prev;
            if diff > 0.0 {
                total_ascent += diff;
            } else {
                total_descent += diff.abs();
            }
        }
    }

    Ok(ElevationProfile {
        elevations,
        min_elevation,
        max_elevation,
        total_ascent,
        total_descent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooths_outliers() {
        let raw = vec![Some(300.0), Some(305.0), Some(400.0), Some(307.0)];
        let path = vec![
            Coordinate { lat: 0.0, lon: 0.0 },
            Coordinate {
                lat: 0.0,
                lon: 0.0001,
            },
            Coordinate {
                lat: 0.0,
                lon: 0.0002,
            },
            Coordinate {
                lat: 0.0,
                lon: 0.0003,
            },
        ];
        let smoothed = super::smooth_elevation_profile(&path, &raw);
        assert_eq!(smoothed.len(), raw.len());
        // Middle spike should be clamped close to neighbours (<= prev + MAX_DELTA)
        assert!(smoothed[2].unwrap() < 340.0);
    }

    #[test]
    fn test_median_empty() {
        let mut values = vec![];
        assert_eq!(median(&mut values), None);
    }

    #[test]
    fn test_median_single() {
        let mut values = vec![42.0];
        assert_eq!(median(&mut values), Some(42.0));
    }

    #[test]
    fn test_median_odd_count() {
        let mut values = vec![3.0, 1.0, 5.0, 2.0, 4.0];
        assert_eq!(median(&mut values), Some(3.0));
    }

    #[test]
    fn test_median_even_count() {
        let mut values = vec![1.0, 4.0, 3.0, 2.0];
        // For even count, we return the element at len/2 after sorting
        // Sorted: [1.0, 2.0, 3.0, 4.0], len/2 = 2, so values[2] = 3.0
        assert_eq!(median(&mut values), Some(3.0));
    }

    #[test]
    fn test_median_with_duplicates() {
        let mut values = vec![5.0, 5.0, 5.0, 5.0];
        assert_eq!(median(&mut values), Some(5.0));
    }

    #[test]
    fn test_haversine_zero_distance() {
        let dist = haversine_m(45.0, 5.0, 45.0, 5.0);
        assert!(dist.abs() < 0.01);
    }

    #[test]
    fn test_haversine_1km_north() {
        // 1km north ≈ 0.009° at any latitude
        let dist = haversine_m(45.0, 5.0, 45.009, 5.0);
        assert!((dist - 1000.0).abs() < 10.0); // Within 10m
    }

    #[test]
    fn test_haversine_1km_east() {
        // 1km east at 45° latitude
        let dist = haversine_m(45.0, 5.0, 45.0, 5.0127);
        assert!((dist - 1000.0).abs() < 50.0); // Within 50m (projection approximation)
    }

    #[test]
    fn test_haversine_symmetry() {
        let dist1 = haversine_m(45.0, 5.0, 46.0, 6.0);
        let dist2 = haversine_m(46.0, 6.0, 45.0, 5.0);
        assert!((dist1 - dist2).abs() < 0.01);
    }

    #[test]
    fn test_haversine_known_distance() {
        // Paris (48.8566, 2.3522) to London (51.5074, -0.1278)
        // Known distance: ~343 km
        let dist = haversine_m(48.8566, 2.3522, 51.5074, -0.1278);
        assert!((dist - 343_000.0).abs() < 5_000.0); // Within 5km
    }

    #[test]
    fn test_smooth_elevation_empty() {
        let path = vec![];
        let raw = vec![];
        let smoothed = smooth_elevation_profile(&path, &raw);
        assert_eq!(smoothed.len(), 0);
    }

    #[test]
    fn test_smooth_elevation_single_point() {
        let path = vec![Coordinate { lat: 0.0, lon: 0.0 }];
        let raw = vec![Some(100.0)];
        let smoothed = smooth_elevation_profile(&path, &raw);
        assert_eq!(smoothed.len(), 1);
        assert_eq!(smoothed[0], Some(100.0));
    }

    #[test]
    fn test_smooth_elevation_no_outliers() {
        let path = vec![
            Coordinate { lat: 0.0, lon: 0.0 },
            Coordinate {
                lat: 0.0,
                lon: 0.0001,
            },
            Coordinate {
                lat: 0.0,
                lon: 0.0002,
            },
        ];
        let raw = vec![Some(100.0), Some(105.0), Some(110.0)];
        let smoothed = smooth_elevation_profile(&path, &raw);

        // Should smooth values (median + clamping), length preserved
        assert_eq!(smoothed.len(), 3);
        // First value starts with median of first 2 values
        assert!(smoothed[0].is_some());
        // All values should be in reasonable range
        for val in &smoothed {
            let v = val.unwrap();
            assert!(v >= 95.0 && v <= 115.0);
        }
    }

    #[test]
    fn test_smooth_elevation_handles_none() {
        let path = vec![
            Coordinate { lat: 0.0, lon: 0.0 },
            Coordinate {
                lat: 0.0,
                lon: 0.0001,
            },
            Coordinate {
                lat: 0.0,
                lon: 0.0002,
            },
        ];
        let raw = vec![Some(100.0), None, Some(110.0)];
        let smoothed = smooth_elevation_profile(&path, &raw);

        // Should handle None gracefully by using median of neighbors
        assert_eq!(smoothed.len(), 3);
        assert_eq!(smoothed[0], Some(100.0));
        assert!(smoothed[1].is_some());
        assert_eq!(smoothed[2], Some(110.0));
    }

    #[test]
    fn test_smooth_elevation_gradual_ascent() {
        let path = vec![
            Coordinate { lat: 0.0, lon: 0.0 },
            Coordinate {
                lat: 0.0,
                lon: 0.0001,
            },
            Coordinate {
                lat: 0.0,
                lon: 0.0002,
            },
            Coordinate {
                lat: 0.0,
                lon: 0.0003,
            },
        ];
        let raw = vec![Some(100.0), Some(110.0), Some(120.0), Some(130.0)];
        let smoothed = smooth_elevation_profile(&path, &raw);

        // Smoothing applies clamping, so verify length and reasonable range
        assert_eq!(smoothed.len(), 4);
        // All values should be defined and in the original range
        for val in &smoothed {
            assert!(val.is_some());
            let v = val.unwrap();
            assert!(v >= 95.0 && v <= 135.0);
        }
        // First and last values should show overall ascent trend
        assert!(smoothed.last().unwrap().unwrap() > smoothed.first().unwrap().unwrap());
    }

    #[tokio::test]
    async fn test_create_elevation_profile_empty_path() {
        let path = vec![];
        let result = create_elevation_profile(&path).await;

        assert!(result.is_ok());
        let profile = result.unwrap();
        assert_eq!(profile.elevations.len(), 0);
        assert_eq!(profile.min_elevation, None);
        assert_eq!(profile.max_elevation, None);
        assert_eq!(profile.total_ascent, 0.0);
        assert_eq!(profile.total_descent, 0.0);
    }
}
