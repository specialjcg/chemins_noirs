/**
 * Caméra du survol, posée en mètres autour du point de lecture : distance,
 * hauteur angulaire au-dessus de l'horizon, angle autour du point.
 * Module pur, testé sous Node.
 */
import { destination, toRadians } from './flyover_track.js';

export const DISTANCE_MIN_M = 200;
export const DISTANCE_MAX_M = 8000;
export const ELEVATION_MIN_DEG = 5;
export const ELEVATION_MAX_DEG = 85;
/** Le curseur de distance va de 0 à cette valeur, sur une échelle logarithmique. */
export const DISTANCE_SLIDER_MAX = 1000;
/** Identifiant de la caméra dès qu'on la règle à la main. */
export const FREE_CAMERA_ID = 'libre';

/** Le cap vise un point plus loin sur le tracé que la caméra n'est éloignée. */
const LOOK_AHEAD_FACTOR = 1.5;
const LOOK_AHEAD_MIN_M = 500;
const LOOK_AHEAD_MAX_M = 3000;
/** Caméra jamais exactement sur le point : MapLibre ne saurait où regarder. */
const MIN_SETBACK_M = 1;

/** Vues de départ, réglables ensuite. */
export const PRESETS = [
  { id: 'derriere', label: 'Derrière', distanceM: 1500, elevationDeg: 30, yawDeg: 0 },
  { id: 'cote', label: 'Côté', distanceM: 1200, elevationDeg: 20, yawDeg: 90 },
  { id: 'rase', label: 'Rase-mottes', distanceM: 350, elevationDeg: 10, yawDeg: 0 },
  { id: 'haut', label: 'Haut', distanceM: 4000, elevationDeg: 45, yawDeg: 0 },
  { id: 'dessus', label: 'Dessus', distanceM: 3000, elevationDeg: 85, yawDeg: 0 }
];

const clamp = (value, min, max) => Math.min(Math.max(value, min), max);

export function constrainCamera(camera) {
  return {
    ...camera,
    distanceM: clamp(camera.distanceM, DISTANCE_MIN_M, DISTANCE_MAX_M),
    elevationDeg: clamp(camera.elevationDeg, ELEVATION_MIN_DEG, ELEVATION_MAX_DEG)
  };
}

export function lookAheadM(camera) {
  return clamp(camera.distanceM * LOOK_AHEAD_FACTOR, LOOK_AHEAD_MIN_M, LOOK_AHEAD_MAX_M);
}

/** Position au sol de la caméra et sa hauteur au-dessus du point visé. */
export function cameraPlacement(target, headingDeg, camera) {
  const elevationRad = toRadians(camera.elevationDeg);
  const setbackM = Math.max(camera.distanceM * Math.cos(elevationRad), MIN_SETBACK_M);
  return {
    position: destination(target, headingDeg + 180 + camera.yawDeg, setbackM),
    heightM: camera.distanceM * Math.sin(elevationRad)
  };
}

const DISTANCE_RANGE_LOG = Math.log(DISTANCE_MAX_M / DISTANCE_MIN_M);

export function distanceToSlider(distanceM) {
  return Math.round(DISTANCE_SLIDER_MAX * Math.log(distanceM / DISTANCE_MIN_M) / DISTANCE_RANGE_LOG);
}

export function sliderToDistance(value) {
  return DISTANCE_MIN_M * (DISTANCE_MAX_M / DISTANCE_MIN_M) ** (value / DISTANCE_SLIDER_MAX);
}
