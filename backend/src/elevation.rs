use serde::{Deserialize, Serialize};
use shared::{Coordinate, ElevationProfile};
use std::error::Error;

const OPEN_METEO_ELEVATION_API: &str = "https://api.open-meteo.com/v1/elevation";
const BATCH_SIZE: usize = 100; // Open-Meteo recommends keeping lists short; we also stay under URL limits

#[derive(Debug, Serialize)]
struct ElevationRequest {
    locations: Vec<Location>,
}

#[derive(Debug, Serialize)]
struct Location {
    latitude: f64,
    longitude: f64,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoElevationResponse {
    elevation: Option<Vec<f64>>,
}

/// Fetch elevation data for a batch of coordinates
pub async fn fetch_elevations(coords: Vec<(f64, f64)>) -> Result<Vec<f64>, Box<dyn Error>> {
    if coords.is_empty() {
        return Ok(Vec::new());
    }

    let client = reqwest::Client::new();
    let mut all_elevations = Vec::with_capacity(coords.len());
    let max_retries = 2;

    // Process in batches to respect API limits
    for chunk in coords.chunks(BATCH_SIZE) {
        // Open-Meteo expects comma-separated lists in the URL
        let (latitudes, longitudes): (Vec<String>, Vec<String>) = chunk
            .iter()
            .map(|(lat, lon)| (format!("{:.6}", lat), format!("{:.6}", lon)))
            .unzip();
        let url = format!(
            "{OPEN_METEO_ELEVATION_API}?latitude={}&longitude={}",
            latitudes.join(","),
            longitudes.join(",")
        );

        tracing::info!(
            "Fetching elevations (Open-Meteo) for {} coordinates...",
            chunk.len()
        );

        let mut attempt = 0;
        loop {
            let response = client.get(&url).send().await?;

            if response.status().is_success() {
                let elevation_response: OpenMeteoElevationResponse = response.json().await?;
                let elevations = elevation_response
                    .elevation
                    .ok_or_else(|| "Elevation API responded without elevations field")?;

                all_elevations.extend(elevations);
                break;
            }

            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            // Retry on 429/5xx with small backoff
            if (status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error())
                && attempt < max_retries
            {
                attempt += 1;
                let wait_ms = 500 * attempt;
                tracing::warn!(
                    "Elevation batch retry {}/{} after {}: {}, waiting {}ms",
                    attempt,
                    max_retries,
                    status,
                    error_text,
                    wait_ms
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(wait_ms as u64)).await;
                continue;
            }

            return Err(format!("Elevation API error {}: {}", status, error_text).into());
        }

        // Small delay between batches to be nice to the API
        if coords.len() > BATCH_SIZE {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    Ok(all_elevations)
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
            let max_delta = (dist_m * 0.2).max(8.0).min(30.0); // meters
            candidate = Some(current.clamp(prev - max_delta, prev + max_delta));
        }

        smoothed.push(candidate);
    }

    smoothed
}

/// Create an elevation profile for a route path
/// Fetches elevations from the API for all coordinates
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

    // Fetch elevations from API
    let elevations_vec = fetch_elevations(coords).await?;
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

    #[tokio::test]
    async fn test_fetch_elevation_single() {
        // Test with Lyon coordinates
        let coords = vec![(45.764, 4.835)];
        let result = fetch_elevations(coords).await;

        assert!(result.is_ok());
        let elevations = result.unwrap();
        assert_eq!(elevations.len(), 1);
        // Lyon is around 170-200m elevation
        assert!(elevations[0] > 100.0 && elevations[0] < 300.0);
    }

    #[tokio::test]
    async fn test_fetch_elevation_multiple() {
        // Test with multiple coordinates in Rhone-Alpes
        let coords = vec![
            (45.764, 4.835),  // Lyon
            (45.9305, 4.577), // Villefranche
        ];
        let result = fetch_elevations(coords).await;

        assert!(result.is_ok());
        let elevations = result.unwrap();
        assert_eq!(elevations.len(), 2);
    }

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
}
