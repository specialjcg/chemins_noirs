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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub message: String,
}

pub fn default_weight() -> f64 {
    1.0
}
