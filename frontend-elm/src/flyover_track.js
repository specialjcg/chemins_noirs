/**
 * Tracé du survol : géométrie sphérique, profil en long et fins d'étape.
 *
 * Module pur, sans carte ni DOM, testé sous Node. Un tracé (`track`) ne change
 * jamais : une donnée nouvelle en fait un autre, ce qui suffit aux appelants
 * pour savoir quand redessiner.
 */

const EARTH_RADIUS_M = 6371000;
/** Une fin d'étape à moins de ça de l'arrivée ferait doublon avec elle. */
const STOP_END_MARGIN_M = 50;
/** Fenêtre sur laquelle la pente affichée est moyennée. */
const SLOPE_WINDOW_M = 200;
/** Évite une division par zéro sur deux points confondus. */
const MIN_SEGMENT_M = 1e-6;
const KM = 1000;

export const toRadians = degrees => degrees * Math.PI / 180;
export const toDegrees = radians => radians * 180 / Math.PI;

const clamp = (value, min, max) => Math.min(Math.max(value, min), max);

// ── Géométrie ────────────────────────────────────────────────────────────

export function normalizeDegrees(angle) {
  return ((angle % 360) + 360) % 360;
}

export function haversine(from, to) {
  const deltaLat = toRadians(to.lat - from.lat);
  const deltaLon = toRadians(to.lon - from.lon);
  const chord = Math.sin(deltaLat / 2) ** 2
    + Math.cos(toRadians(from.lat)) * Math.cos(toRadians(to.lat)) * Math.sin(deltaLon / 2) ** 2;
  return 2 * EARTH_RADIUS_M * Math.asin(Math.sqrt(chord));
}

export function bearing(from, to) {
  const deltaLon = toRadians(to.lon - from.lon);
  const east = Math.sin(deltaLon) * Math.cos(toRadians(to.lat));
  const north = Math.cos(toRadians(from.lat)) * Math.sin(toRadians(to.lat))
    - Math.sin(toRadians(from.lat)) * Math.cos(toRadians(to.lat)) * Math.cos(deltaLon);
  return normalizeDegrees(toDegrees(Math.atan2(east, north)));
}

export function destination(origin, bearingDeg, distanceM) {
  const angularDistance = distanceM / EARTH_RADIUS_M;
  const bearingRad = toRadians(bearingDeg);
  const originLat = toRadians(origin.lat);
  const lat = Math.asin(
    Math.sin(originLat) * Math.cos(angularDistance)
    + Math.cos(originLat) * Math.sin(angularDistance) * Math.cos(bearingRad)
  );
  const lon = toRadians(origin.lon) + Math.atan2(
    Math.sin(bearingRad) * Math.sin(angularDistance) * Math.cos(originLat),
    Math.cos(angularDistance) - Math.sin(originLat) * Math.sin(lat)
  );
  return { lat: toDegrees(lat), lon: toDegrees(lon) };
}

// ── Lissage ──────────────────────────────────────────────────────────────

/** Part du chemin à faire en `elapsedS` pour un lissage exponentiel de constante `timeConstantS`. */
export function smoothingFactor(elapsedS, timeConstantS) {
  return 1 - Math.exp(-elapsedS / timeConstantS);
}

/** Rapproche `current` de `target` ; sans valeur courante, on part de la cible. */
export function approach(current, target, factor) {
  return current === null ? target : current + (target - current) * factor;
}

/** Comme `approach` pour un cap, en tournant du côté le plus court. */
export function smoothAngle(currentDeg, targetDeg, factor) {
  if (currentDeg === null) return targetDeg;
  const shortestTurn = normalizeDegrees(targetDeg - currentDeg + 180) - 180;
  return normalizeDegrees(currentDeg + shortestTurn * factor);
}

// ── Tracé ────────────────────────────────────────────────────────────────

/** coords : [{lat, lon}] ; `cumul` donne la distance depuis le départ de chaque point. */
export function createTrack(coords) {
  const points = Array.isArray(coords) ? coords.map(({ lat, lon }) => ({ lat, lon })) : [];
  const cumul = [0];
  for (let index = 1; index < points.length; index++) {
    cumul.push(cumul[index - 1] + haversine(points[index - 1], points[index]));
  }
  return { points, cumul, totalM: cumul[cumul.length - 1] || 0, elevations: null };
}

/** Altitudes alignées sur les points ; un profil d'une autre longueur est ignoré. */
export function withElevations(track, elevations) {
  const fits = Array.isArray(elevations) && elevations.length === track.points.length;
  return fits ? { ...track, elevations } : track;
}

/** Indice du segment contenant la distance (recherche dichotomique). */
function segmentAt(track, distanceM) {
  let low = 0;
  let high = track.cumul.length - 1;
  while (high - low > 1) {
    const middle = (low + high) >> 1;
    if (track.cumul[middle] <= distanceM) low = middle; else high = middle;
  }
  return low;
}

export function pointAt(track, distanceM) {
  const { points, cumul } = track;
  if (distanceM <= 0) return points[0];
  if (distanceM >= track.totalM) return points[points.length - 1];
  const segment = segmentAt(track, distanceM);
  const progress = (distanceM - cumul[segment]) / Math.max(cumul[segment + 1] - cumul[segment], MIN_SEGMENT_M);
  const start = points[segment];
  const end = points[segment + 1];
  return {
    lat: start.lat + (end.lat - start.lat) * progress,
    lon: start.lon + (end.lon - start.lon) * progress
  };
}

export function elevationAt(track, distanceM) {
  const { elevations } = track;
  if (!elevations) return null;
  const segment = segmentAt(track, clamp(distanceM, 0, track.totalM));
  return elevations[segment] ?? elevations[segment + 1] ?? null;
}

/** Montée cumulée entre deux distances. */
export function ascentBetween(track, fromM, toM) {
  const { elevations, cumul } = track;
  if (!elevations) return null;
  let ascent = 0;
  let previous = null;
  for (let index = segmentAt(track, fromM); index < cumul.length && cumul[index] <= toM; index++) {
    const elevation = elevations[index];
    if (elevation == null) continue;
    if (previous !== null && elevation > previous) ascent += elevation - previous;
    previous = elevation;
  }
  return ascent;
}

/** Pente moyenne autour du point, en %. */
export function slopeAt(track, distanceM) {
  const fromM = Math.max(distanceM - SLOPE_WINDOW_M / 2, 0);
  const toM = Math.min(distanceM + SLOPE_WINDOW_M / 2, track.totalM);
  const fromElevation = elevationAt(track, fromM);
  const toElevation = elevationAt(track, toM);
  if (fromElevation == null || toElevation == null || toM <= fromM) return null;
  return (toElevation - fromElevation) / (toM - fromM) * 100;
}

// ── Fins d'étape ─────────────────────────────────────────────────────────

const isInnerStop = (stop, track) => stop.distanceM > 0 && stop.distanceM < track.totalM - STOP_END_MARGIN_M;

/** Étapes du planificateur : [{ endKm, title, detail }]. */
export function stopsFromStages(stages, track) {
  return stages
    .map(stage => ({ distanceM: stage.endKm * KM, title: stage.title, detail: stage.detail }))
    .filter(stop => isInnerStop(stop, track));
}

function nearestPointIndex(track, position) {
  let nearest = 0;
  let nearestM = Infinity;
  track.points.forEach((point, index) => {
    const distanceM = haversine(position, point);
    if (distanceM < nearestM) {
      nearestM = distanceM;
      nearest = index;
    }
  });
  return nearest;
}

/** Points de passage intermédiaires, rangés dans l'ordre du tracé. */
export function stopsFromWaypoints(waypoints, track) {
  return waypoints
    .slice(1, -1)
    // Le départ est le point 1, le premier intermédiaire le point 2.
    .map((waypoint, innerIndex) => ({
      distanceM: track.cumul[nearestPointIndex(track, waypoint)],
      title: `Point ${innerIndex + 2}`,
      detail: null
    }))
    .filter(stop => isInnerStop(stop, track))
    .sort((first, second) => first.distanceM - second.distanceM);
}
