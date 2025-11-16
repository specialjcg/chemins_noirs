use serde::{Deserialize, Serialize};
use shared::{Coordinate, ElevationProfile};
use std::error::Error;

const OPEN_ELEVATION_API: &str = "https://api.open-elevation.com/api/v1/lookup";
const BATCH_SIZE: usize = 500; // Open-Elevation supports up to 1000, use 500 for safety

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
struct ElevationResponse {
    results: Vec<ElevationResult>,
}

#[derive(Debug, Deserialize)]
struct ElevationResult {
    elevation: f64,
}

/// Fetch elevation data for a batch of coordinates
pub async fn fetch_elevations(coords: Vec<(f64, f64)>) -> Result<Vec<f64>, Box<dyn Error>> {
    if coords.is_empty() {
        return Ok(Vec::new());
    }

    let client = reqwest::Client::new();
    let mut all_elevations = Vec::with_capacity(coords.len());

    // Process in batches to respect API limits
    for chunk in coords.chunks(BATCH_SIZE) {
        let locations: Vec<Location> = chunk
            .iter()
            .map(|(lat, lon)| Location {
                latitude: *lat,
                longitude: *lon,
            })
            .collect();

        let request = ElevationRequest { locations };

        tracing::info!(
            "Fetching elevations for {} coordinates...",
            chunk.len()
        );

        let response = client
            .post(OPEN_ELEVATION_API)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(format!("Elevation API error {}: {}", status, error_text).into());
        }

        let elevation_response: ElevationResponse = response.json().await?;

        for result in elevation_response.results {
            all_elevations.push(result.elevation);
        }

        // Small delay between batches to be nice to the API
        if coords.len() > BATCH_SIZE {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    Ok(all_elevations)
}

/// Create an elevation profile for a route path
/// Fetches elevations from the API for all coordinates
pub async fn create_elevation_profile(path: &[Coordinate]) -> Result<ElevationProfile, Box<dyn Error>> {
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
    let elevations: Vec<Option<f64>> = elevations_vec.into_iter().map(Some).collect();

    // Calculate statistics
    let valid_elevations: Vec<f64> = elevations.iter().filter_map(|&e| e).collect();

    let min_elevation = valid_elevations.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_elevation = valid_elevations.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    let min_elevation = if min_elevation.is_finite() { Some(min_elevation) } else { None };
    let max_elevation = if max_elevation.is_finite() { Some(max_elevation) } else { None };

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
}
