use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use geo_types::Point;
use gpx::{Gpx, GpxVersion, Track, TrackSegment, Waypoint};

use crate::error::RouteError;
use crate::models::Coordinate;

pub fn encode_route_as_gpx(path: &[Coordinate]) -> Result<String, RouteError> {
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
    for waypoint in path.iter().map(to_waypoint) {
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
