use crate::elevation::get_elevations;
use crate::models::RouteBounds;
use serde_json;
use shared::{Coordinate, TerrainMesh};
use std::fs;
use std::future::Future;
use std::io;
use std::path::PathBuf;

const METERS_PER_DEGREE_LAT: f64 = 111_000.0;
const SCALE_FACTOR: f64 = 10.0;
const ELEVATION_SCALE: f64 = 6.0; // natural human-scale relief
const SEGMENTS: u32 = 64; // 64x64 = 4225 vertices for smoother relief (DEM local)

fn terrain_cache_dir() -> PathBuf {
    PathBuf::from("backend/data/terrain_cache")
}

fn sanitize_component(val: f64) -> String {
    let s = format!("{val:.4}");
    s.replace('-', "m").replace('.', "p")
}

fn cache_path(bounds: &RouteBounds) -> PathBuf {
    let file = format!(
        "{}_{}_{}_{}_s{}.json",
        sanitize_component(bounds.min_lat),
        sanitize_component(bounds.max_lat),
        sanitize_component(bounds.min_lon),
        sanitize_component(bounds.max_lon),
        SEGMENTS
    );
    terrain_cache_dir().join(file)
}

fn try_load_cache(bounds: &RouteBounds) -> io::Result<TerrainMesh> {
    let path = cache_path(bounds);
    let content = fs::read_to_string(path)?;
    let mesh: TerrainMesh = serde_json::from_str(&content)?;
    Ok(mesh)
}

fn persist_cache(bounds: &RouteBounds, mesh: &TerrainMesh) -> io::Result<()> {
    let dir = terrain_cache_dir();
    fs::create_dir_all(&dir)?;
    let path = cache_path(bounds);
    let data = serde_json::to_string(mesh)?;
    fs::write(path, data)?;
    Ok(())
}

/// Build a 3D terrain mesh (positions, uvs, indices) around the route.
pub async fn build_terrain_mesh(
    path: &[Coordinate],
) -> Result<TerrainMesh, Box<dyn std::error::Error>> {
    build_terrain_mesh_with_fetch(path, get_elevations).await
}

async fn build_terrain_mesh_with_fetch<F, Fut>(
    path: &[Coordinate],
    fetch_fn: F,
) -> Result<TerrainMesh, Box<dyn std::error::Error>>
where
    F: Fn(Vec<(f64, f64)>) -> Fut,
    Fut: Future<Output = Result<Vec<f64>, Box<dyn std::error::Error>>>,
{
    if path.is_empty() {
        return Err("empty path".into());
    }

    // Compute bounds with padding
    let mut min_lat = f64::MAX;
    let mut max_lat = f64::MIN;
    let mut min_lon = f64::MAX;
    let mut max_lon = f64::MIN;
    for c in path {
        min_lat = min_lat.min(c.lat);
        max_lat = max_lat.max(c.lat);
        min_lon = min_lon.min(c.lon);
        max_lon = max_lon.max(c.lon);
    }
    let lat_padding = (max_lat - min_lat) * 0.3;
    let lon_padding = (max_lon - min_lon) * 0.3;

    let padded_bounds = RouteBounds {
        min_lat: min_lat - lat_padding,
        max_lat: max_lat + lat_padding,
        min_lon: min_lon - lon_padding,
        max_lon: max_lon + lon_padding,
    };

    if let Ok(mesh) = try_load_cache(&padded_bounds) {
        tracing::info!("Terrain cache hit");
        return Ok(mesh);
    }

    tracing::info!(
        "Building terrain mesh {}x{} ({} vertices) for bounds {:?}",
        SEGMENTS + 1,
        SEGMENTS + 1,
        (SEGMENTS as usize + 1) * (SEGMENTS as usize + 1),
        padded_bounds
    );

    // Grid sampling in lat/lon
    let steps = SEGMENTS as usize;
    let total_lat = padded_bounds.max_lat - padded_bounds.min_lat;
    let total_lon = padded_bounds.max_lon - padded_bounds.min_lon;

    let mut samples = Vec::with_capacity((steps + 1) * (steps + 1));
    for y in 0..=steps {
        let t_lat = y as f64 / steps as f64;
        let lat = padded_bounds.min_lat + total_lat * t_lat;
        for x in 0..=steps {
            let t_lon = x as f64 / steps as f64;
            let lon = padded_bounds.min_lon + total_lon * t_lon;
            samples.push((lat, lon));
        }
    }

    let elevations = fetch_fn(samples).await?;
    if elevations.len() != (steps + 1) * (steps + 1) {
        return Err("elevation vector size mismatch".into());
    }

    // Convert to positions/uvs
    let center_lat = (min_lat + max_lat) / 2.0;
    let center_lon = (min_lon + max_lon) / 2.0;

    let mut positions = Vec::with_capacity(elevations.len() * 3);
    let mut uvs = Vec::with_capacity(elevations.len() * 2);
    let mut min_elev = f64::MAX;
    let mut max_elev = f64::MIN;

    for (idx, elevation) in elevations.iter().enumerate() {
        let row = idx / (steps + 1);
        let col = idx % (steps + 1);

        let t_lon = col as f64 / steps as f64;
        let t_lat = row as f64 / steps as f64;

        let lat = padded_bounds.min_lat + total_lat * t_lat;
        let lon = padded_bounds.min_lon + total_lon * t_lon;

        let x = (lon - center_lon) * METERS_PER_DEGREE_LAT * center_lat.to_radians().cos()
            / SCALE_FACTOR;
        let z = -(lat - center_lat) * METERS_PER_DEGREE_LAT / SCALE_FACTOR;
        let y = elevation * ELEVATION_SCALE / SCALE_FACTOR;

        positions.push(x as f32);
        positions.push(y as f32);
        positions.push(z as f32);

        uvs.push(t_lon as f32);
        uvs.push(t_lat as f32);

        min_elev = min_elev.min(*elevation);
        max_elev = max_elev.max(*elevation);
    }

    // Indices
    let mut indices = Vec::with_capacity(steps * steps * 6);
    let stride = steps + 1;
    for y in 0..steps {
        for x in 0..steps {
            let i0 = (y * stride + x) as u32;
            let i1 = i0 + 1;
            let i2 = i0 + stride as u32;
            let i3 = i2 + 1;
            indices.extend_from_slice(&[i0, i2, i1, i1, i2, i3]);
        }
    }

    let mesh = TerrainMesh {
        positions,
        uvs,
        indices,
        min_elevation: min_elev as f32,
        max_elevation: max_elev as f32,
        center_lat,
        center_lon,
        scale_factor: SCALE_FACTOR as f32,
        elevation_scale: ELEVATION_SCALE as f32,
        bounds: padded_bounds.clone(),
        segments: SEGMENTS,
    };

    if let Err(err) = persist_cache(&padded_bounds, &mesh) {
        tracing::warn!("Failed to persist terrain cache: {}", err);
    }

    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn builds_mesh_with_mock_elevation() {
        let path = vec![
            Coordinate {
                lat: 45.0,
                lon: 4.0,
            },
            Coordinate {
                lat: 45.001,
                lon: 4.001,
            },
        ];

        let fetch = |coords: Vec<(f64, f64)>| async move {
            Ok(coords
                .iter()
                .enumerate()
                .map(|(idx, _)| idx as f64)
                .collect::<Vec<_>>())
        };

        let mesh = build_terrain_mesh_with_fetch(&path, fetch).await.unwrap();
        let vertex_count = (SEGMENTS as usize + 1) * (SEGMENTS as usize + 1);

        assert_eq!(mesh.positions.len(), vertex_count * 3);
        assert_eq!(mesh.uvs.len(), vertex_count * 2);
        assert_eq!(
            mesh.indices.len(),
            (SEGMENTS as usize) * (SEGMENTS as usize) * 6
        );
        assert!(mesh.max_elevation > mesh.min_elevation);
    }
}
