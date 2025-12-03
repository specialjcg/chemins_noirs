use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Coordinate {
    pub lat: f64,
    pub lon: f64,
}

impl Coordinate {
    pub fn interpolate(self, other: Self, t: f64) -> Self {
        Self {
            lat: self.lat + (other.lat - self.lat) * t,
            lon: self.lon + (other.lon - self.lon) * t,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceType {
    Paved,
    Trail,
    Dirt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRequest {
    pub start: Coordinate,
    pub end: Coordinate,
    #[serde(default = "default_weight")]
    pub w_pop: f64,
    #[serde(default = "default_weight")]
    pub w_paved: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRouteRequest {
    pub start: Coordinate,
    pub target_distance_km: f64,
    #[serde(default = "default_distance_tolerance_km")]
    pub distance_tolerance_km: f64,
    #[serde(default = "default_loop_candidate_count")]
    pub candidate_count: usize,
    #[serde(default = "default_weight")]
    pub w_pop: f64,
    #[serde(default = "default_weight")]
    pub w_paved: f64,
    #[serde(default)]
    pub max_total_ascent: Option<f64>,
    #[serde(default)]
    pub min_total_ascent: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteMetadata {
    pub point_count: usize,
    pub bounds: RouteBounds,
    pub start: Coordinate,
    pub end: Coordinate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteBounds {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElevationProfile {
    pub elevations: Vec<Option<f64>>, // Elevation in meters for each point in path
    pub min_elevation: Option<f64>,
    pub max_elevation: Option<f64>,
    pub total_ascent: f64,  // Total meters climbed
    pub total_descent: f64, // Total meters descended
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteResponse {
    pub path: Vec<Coordinate>,
    pub distance_km: f64,
    pub gpx_base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<RouteMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation_profile: Option<ElevationProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terrain: Option<TerrainMesh>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerrainMesh {
    pub positions: Vec<f32>, // flat array xyz
    pub uvs: Vec<f32>,       // flat array uv
    pub indices: Vec<u32>,   // triangle indices
    pub min_elevation: f32,
    pub max_elevation: f32,
    pub center_lat: f64,
    pub center_lon: f64,
    pub scale_factor: f32,
    pub elevation_scale: f32,
    pub bounds: RouteBounds, // padded bounds used for mesh
    pub segments: u32,       // segments per axis
}

pub fn default_weight() -> f64 {
    1.0
}

pub fn default_loop_candidate_count() -> usize {
    6
}

pub fn default_distance_tolerance_km() -> f64 {
    1.5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRouteResponse {
    pub target_distance_km: f64,
    pub distance_tolerance_km: f64,
    pub candidates: Vec<LoopCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopCandidate {
    pub route: RouteResponse,
    pub distance_error_km: f64,
    pub bearing_deg: f64,
}
