use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use geo_types::Point;
use gpx::{Gpx, GpxVersion, Track, TrackSegment, Waypoint};

use crate::error::RouteError;
use crate::models::Coordinate;

pub fn encode_route_as_gpx(path: &[Coordinate]) -> Result<String, RouteError> {
    encode_route_as_gpx_with_elevations(path, None)
}

/// Comme `encode_route_as_gpx`, en portant les altitudes dans les points.
///
/// Sans elles un GPX ne montre aucun profil une fois chargé dans une montre ou
/// une application de rando : la trace est juste, mais le dénivelé disparaît.
pub fn encode_route_as_gpx_with_elevations(
    path: &[Coordinate],
    elevations: Option<&[Option<f64>]>,
) -> Result<String, RouteError> {
    let mut gpx = Gpx {
        version: GpxVersion::Gpx11,
        creator: Some("chemins_noirs".into()),
        ..Default::default()
    };
    let mut track = Track {
        name: Some("chemins_noirs".into()),
        ..Default::default()
    };

    let mut segment = TrackSegment::new();
    for (idx, coord) in path.iter().enumerate() {
        let mut waypoint = to_waypoint(coord);
        waypoint.elevation = elevations.and_then(|e| e.get(idx).copied().flatten());
        segment.points.push(waypoint);
    }
    track.segments.push(segment);
    gpx.tracks.push(track);

    let mut buffer = Vec::new();
    gpx::write(&gpx, &mut buffer)?;
    Ok(BASE64.encode(buffer))
}

fn to_waypoint(coord: &Coordinate) -> Waypoint {
    Waypoint::new(Point::new(coord.lon, coord.lat))
}
