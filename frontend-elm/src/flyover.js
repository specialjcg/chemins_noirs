/**
 * Survol 3D du tracé, à la manière des survols d'étape du Tour de France.
 *
 * Un point de lecture avance sur le tracé ; la caméra est posée en mètres par
 * rapport à lui (voir flyover_camera.js) avec `calculateCameraOptionsFromTo`.
 * L'ancien survol réglait tout par le zoom et des modes figés : impossible de
 * dire « plus haut » ou « plus vite ».
 *
 * Ce module tient l'état de lecture et pilote la caméra ; géométrie, réglages
 * de caméra, commandes, panneau et calques vivent dans les modules flyover_*.
 */
import maplibregl from 'maplibre-gl';
import {
  approach,
  ascentBetween,
  bearing,
  createTrack,
  haversine,
  pointAt,
  smoothAngle,
  smoothingFactor,
  stopsFromStages,
  stopsFromWaypoints,
  withElevations
} from './flyover_track.js';
import { PRESETS, FREE_CAMERA_ID, cameraPlacement, constrainCamera, lookAheadM } from './flyover_camera.js';
import { captureMapInputs } from './flyover_inputs.js';
import { createFlyoverPanel } from './flyover_panel.js';
import { createStageCard, dropProgress, hideProgress, showProgress } from './flyover_overlays.js';

const KM = 1000;
const BASE_SPEED_MS = 100;               // ×1 = 100 m/s, une étape de 25 km en ~4 min
const SPEEDS = [0.5, 1, 2, 4, 8, 16];
const DEFAULT_SPEED = 4;                 // ~1 min par étape de 25 km
const MIN_FLYABLE_M = 10;
const CLEARANCE_M = 30;                  // jamais plus près du sol
const MIN_HEIGHT_ABOVE_TARGET_M = 5;
/** En deçà, le point visé devant est confondu avec le point courant : cap gardé. */
const MIN_HEADING_BASE_M = 1;
const HEADING_TAU_S = 1.5;               // lissage du cap
const ALT_TAU_S = 0.6;                   // lissage des altitudes lues sur le relief
const RATE_TAU_S = 0.4;                  // ralentir et repartir sans à-coup
const STALL_RATE = 0.2;                  // allure pendant qu'on attend les tuiles
const TILE_WAIT_BUDGET_MS = 2000;        // au-delà, on avance même sans tuiles
const MAX_FRAME_DELTA_MS = 100;
const STAGE_PAUSE_MS = 3000;
const PROGRESS_REFRESH_MS = 100;
/** Un point de lecture à moins de ça d'une fin d'étape est considéré dessus. */
const AT_STOP_TOLERANCE_M = 1;
const SEEK_STEP_M = 1000;
const ELEVATION_STEP_DEG = 5;
const STOP_VIEW = { pitch: 45, zoom: 15, durationMs: 1200 };

let map = null;
let deps = null;

// Tracé
let track = createTrack([]);
let stops = [];                          // [{ distanceM, title, detail }] fins d'étape
let stageSource = 'none';
let lastWaypoints = null;                // rejoués quand le tracé arrive après eux

// Lecture
let isOpen = false;
let isPlaying = false;
let frameId = null;
let lastFrameTs = null;
let stalledMs = 0;
let positionM = 0;
let speed = DEFAULT_SPEED;
let camera = { ...PRESETS[0] };
let headingDeg = null;
let nextStopIndex = 0;
let stageTimer = null;
let lastProgressRefresh = 0;
let rate = 1;                            // fraction de la vitesse réellement appliquée
let targetAltitude = null;
let cameraAltitude = null;
let colors = null;                       // jetons CSS lus une fois par ouverture
let releaseInputs = null;

// DOM
let panel = null;
let stageCard = null;
let headMarker = null;

// ── API ──────────────────────────────────────────────────────────────────

/**
 * deps : { enableTerrain(), mapColor(token) }
 */
export function initFlyover(mapInstance, dependencies) {
  map = mapInstance;
  deps = dependencies;
}

export function setFlyoverRoute(coords) {
  stopFlyover();
  track = createTrack(coords);
  stops = [];
  stageSource = 'none';
  positionM = 0;
  if (map) dropProgress(map);
  if (lastWaypoints) setFlyoverWaypoints(lastWaypoints);
}

/** Altitudes alignées sur le tracé ; une liste d'une autre longueur garde les précédentes. */
export function setFlyoverElevations(values) {
  track = withElevations(track, values);
  updatePanel();
}

/** Fins d'étape du planificateur : [{ endKm, title, detail }]. null les retire. */
export function setFlyoverStages(stages) {
  if (!stages) {
    if (stageSource === 'plan') setStops([], 'none');
    return;
  }
  setStops(stopsFromStages(stages, track), 'plan');
}

/** Points de passage : étapes par défaut tant que le planificateur n'a pas tourné. */
export function setFlyoverWaypoints(waypoints) {
  lastWaypoints = Array.isArray(waypoints) ? waypoints : null;
  if (stageSource === 'plan' || track.points.length < 2 || !lastWaypoints) return;
  const waypointStops = stopsFromWaypoints(lastWaypoints, track);
  setStops(waypointStops, waypointStops.length ? 'waypoints' : 'none');
}

export function isFlyoverOpen() {
  return isOpen;
}

/** Ouvre le panneau et lance la lecture. */
export function startFlyover() {
  if (track.points.length < 2 || track.totalM < MIN_FLYABLE_M) {
    console.warn('[flyover] pas de tracé à survoler');
    return;
  }
  if (!isOpen) openFlyover();
  play();
}

/** Ferme le panneau et revient à la vue 3D posée. */
export function stopFlyover() {
  if (!isOpen) return;
  pause();
  isOpen = false;
  hideStageCard();
  panel?.remove();
  panel = null;
  headMarker?.remove();
  headMarker = null;
  if (map) hideProgress(map);
  releaseInputs?.();
  releaseInputs = null;
  map?.easeTo({ pitch: STOP_VIEW.pitch, zoom: STOP_VIEW.zoom, roll: 0, duration: STOP_VIEW.durationMs });
  console.debug('[flyover] fermé');
}

/** Bouton de la barre de contrôles : ouvre/ferme le survol. */
export function createFlyoverControl() {
  class FlyoverControl {
    onAdd() {
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';
      const toggle = document.createElement('button');
      toggle.className = 'maplibregl-ctrl-animation';
      toggle.textContent = '▶';
      toggle.title = 'Survol 3D du tracé';
      toggle.onclick = () => (isOpen ? stopFlyover() : startFlyover());
      this._container.appendChild(toggle);
      return this._container;
    }

    onRemove() {
      this._container.remove();
    }
  }
  return new FlyoverControl();
}

// ── Ouverture ────────────────────────────────────────────────────────────

function openFlyover() {
  isOpen = true;
  colors = {
    done: deps.mapColor('--map-flyover-done'),
    profile: deps.mapColor('--map-flyover-profile'),
    stop: deps.mapColor('--map-flyover-stop'),
    cursor: deps.mapColor('--map-flyover-cursor')
  };
  positionM = 0;
  resetSmoothing();
  resetNextStop();
  releaseInputs = captureMapInputs(map, {
    zoom: factor => setCamera({ distanceM: camera.distanceM * factor, id: FREE_CAMERA_ID }),
    orbit: (yawDeg, elevationDeg) => setCamera({
      yawDeg: camera.yawDeg + yawDeg,
      elevationDeg: camera.elevationDeg + elevationDeg,
      id: FREE_CAMERA_ID
    }),
    keys: keyActions()
  });
  panel = createFlyoverPanel(map.getContainer(), {
    togglePlay,
    changeSpeed,
    setCamera,
    seekFraction: fraction => seek(fraction * track.totalM),
    close: stopFlyover
  });
  updatePanel();
}

function keyActions() {
  return {
    ' ': togglePlay,
    ArrowRight: () => seek(positionM + SEEK_STEP_M),
    ArrowLeft: () => seek(positionM - SEEK_STEP_M),
    ArrowUp: () => setCamera({ elevationDeg: camera.elevationDeg + ELEVATION_STEP_DEG, id: FREE_CAMERA_ID }),
    ArrowDown: () => setCamera({ elevationDeg: camera.elevationDeg - ELEVATION_STEP_DEG, id: FREE_CAMERA_ID }),
    '+': () => changeSpeed(1),
    '=': () => changeSpeed(1),
    '-': () => changeSpeed(-1),
    Escape: stopFlyover
  };
}

function updatePanel() {
  panel?.update({
    isPlaying: isPlaying || stageTimer !== null,
    speed,
    positionM,
    track,
    stops,
    camera,
    colors
  });
}

// ── Lecture ──────────────────────────────────────────────────────────────

function setStops(nextStops, source) {
  stops = nextStops;
  stageSource = source;
  resetNextStop();
  updatePanel();
}

function resetNextStop() {
  nextStopIndex = stops.findIndex(stop => stop.distanceM > positionM + AT_STOP_TOLERANCE_M);
  if (nextStopIndex < 0) nextStopIndex = stops.length;
}

function resetSmoothing() {
  headingDeg = null;
  targetAltitude = null;
  cameraAltitude = null;
}

function play() {
  if (isPlaying) return;
  if (positionM >= track.totalM - AT_STOP_TOLERANCE_M) seek(0);
  deps.enableTerrain();
  hideStageCard();
  isPlaying = true;
  lastFrameTs = null;
  stalledMs = 0;
  rate = 1;
  frameId = requestAnimationFrame(frame);
  updatePanel();
  console.debug('[flyover] lecture', `${(track.totalM / KM).toFixed(1)} km`, `×${speed}`, camera.id);
}

function pause() {
  isPlaying = false;
  if (frameId !== null) cancelAnimationFrame(frameId);
  frameId = null;
  clearTimeout(stageTimer);
  stageTimer = null;
  updatePanel();
}

function togglePlay() {
  if (isPlaying || stageTimer) pause(); else play();
}

function seek(distanceM) {
  positionM = Math.min(Math.max(distanceM, 0), track.totalM);
  resetSmoothing();
  resetNextStop();
  hideStageCard();
  snapCamera();
}

function changeSpeed(step) {
  const index = SPEEDS.indexOf(speed);
  speed = SPEEDS[Math.min(Math.max(index + step, 0), SPEEDS.length - 1)];
  updatePanel();
}

function setCamera(changes) {
  camera = constrainCamera({ ...camera, ...changes });
  if (!isPlaying) snapCamera();
  updatePanel();
}

function frame(timestamp) {
  const deltaMs = lastFrameTs === null ? 0 : Math.min(timestamp - lastFrameTs, MAX_FRAME_DELTA_MS);
  lastFrameTs = timestamp;
  advance(deltaMs);

  const nextStop = stops[nextStopIndex];
  if (nextStop && positionM >= nextStop.distanceM) {
    positionM = nextStop.distanceM;
    snapCamera();
    stageBreak(nextStop);
    nextStopIndex++;
    return;
  }
  if (positionM >= track.totalM) {
    positionM = track.totalM;
    snapCamera();
    pause();
    showStageCard({ title: 'Arrivée', detail: null });
    return;
  }

  stepCamera(deltaMs / 1000);
  frameId = requestAnimationFrame(frame);
}

/**
 * Tuiles en retard : on ralentit en douceur au lieu de s'arrêter net, sinon
 * la caméra avance par à-coups ; au-delà du budget, on repart quand même.
 */
function advance(deltaMs) {
  const tilesReady = typeof map.areTilesLoaded !== 'function' || map.areTilesLoaded();
  stalledMs = tilesReady ? 0 : stalledMs + deltaMs;
  const wantedRate = tilesReady || stalledMs > TILE_WAIT_BUDGET_MS ? 1 : STALL_RATE;
  rate = approach(rate, wantedRate, smoothingFactor(deltaMs / 1000, RATE_TAU_S));
  positionM += BASE_SPEED_MS * speed * rate * deltaMs / 1000;
}

/** Pause à une fin d'étape, carte affichée, puis reprise sauf pause manuelle. */
function stageBreak(stop) {
  isPlaying = false;
  frameId = null;
  showStageCard(stop);
  updatePanel();
  stageTimer = setTimeout(() => {
    stageTimer = null;
    play();
  }, STAGE_PAUSE_MS);
}

// ── Caméra ───────────────────────────────────────────────────────────────

const canFly = () => map && track.points.length >= 2;

/** Caméra posée d'un coup : ouverture, saut, arrêt sur étape, réglage. */
function snapCamera() {
  if (!canFly()) return;
  placeCamera({ headingFactor: 1, altitudeFactor: 1 });
  refreshProgress();
}

/** Caméra qui suit le point avec lissage, une image après l'autre. */
function stepCamera(elapsedS) {
  if (!canFly()) return;
  placeCamera({
    headingFactor: smoothingFactor(elapsedS, HEADING_TAU_S),
    altitudeFactor: smoothingFactor(elapsedS, ALT_TAU_S)
  });
  if (performance.now() - lastProgressRefresh > PROGRESS_REFRESH_MS) refreshProgress();
}

function placeCamera({ headingFactor, altitudeFactor }) {
  const target = pointAt(track, positionM);
  headingDeg = smoothAngle(headingDeg, headingFrom(target), headingFactor);

  // Le relief s'affine quand ses tuiles arrivent : lu brut, il fait tressauter
  // la caméra. On lisse les deux altitudes.
  targetAltitude = approach(targetAltitude, groundAltitude(target), altitudeFactor);
  const placement = cameraPlacement(target, headingDeg, camera);
  const wantedCameraAltitude = Math.max(
    targetAltitude + placement.heightM,
    groundAltitude(placement.position) + CLEARANCE_M
  );
  cameraAltitude = approach(cameraAltitude, wantedCameraAltitude, altitudeFactor);

  try {
    const options = map.calculateCameraOptionsFromTo(
      [placement.position.lon, placement.position.lat],
      Math.max(cameraAltitude, targetAltitude + MIN_HEIGHT_ABOVE_TARGET_M),
      [target.lon, target.lat],
      targetAltitude
    );
    map.jumpTo(options);
  } catch (error) {
    // Caméra et cible confondues : rien à faire pour cette image.
  }
  moveHeadMarker(target);
}

/** Cap vers un point plus loin sur le tracé, pour ne pas suivre chaque lacet. */
function headingFrom(target) {
  const ahead = pointAt(track, Math.min(positionM + lookAheadM(camera), track.totalM));
  return haversine(target, ahead) > MIN_HEADING_BASE_M ? bearing(target, ahead) : (headingDeg ?? 0);
}

function groundAltitude(point) {
  const elevation = map.queryTerrainElevation?.([point.lon, point.lat]);
  return Number.isFinite(elevation) ? elevation : 0;
}

function moveHeadMarker(target) {
  if (headMarker) {
    headMarker.setLngLat([target.lon, target.lat]);
    return;
  }
  const dot = document.createElement('div');
  dot.className = 'flyover-head';
  headMarker = new maplibregl.Marker({ element: dot }).setLngLat([target.lon, target.lat]).addTo(map);
}

function refreshProgress() {
  lastProgressRefresh = performance.now();
  showProgress(map, { points: track.points, fraction: positionM / track.totalM, doneColor: colors.done });
  updatePanel();
}

// ── Carte d'étape ────────────────────────────────────────────────────────

function showStageCard(stop) {
  hideStageCard();
  const previousStop = stops.filter(candidate => candidate.distanceM < positionM - AT_STOP_TOLERANCE_M).pop();
  const fromM = previousStop ? previousStop.distanceM : 0;
  const ascent = ascentBetween(track, fromM, positionM);
  const facts = [
    `${((positionM - fromM) / KM).toFixed(1)} km`,
    ascent != null ? `D+ ${Math.round(ascent)} m` : null,
    stop.detail
  ].filter(Boolean).join(' · ');
  stageCard = createStageCard(map.getContainer(), { title: stop.title, facts });
}

function hideStageCard() {
  stageCard?.remove();
  stageCard = null;
}
