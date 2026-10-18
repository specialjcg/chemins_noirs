/**
 * Survol 3D du tracé, à la manière des survols d'étape du Tour de France.
 *
 * Un point de lecture avance sur le tracé ; la caméra est posée en mètres par
 * rapport à lui (distance, hauteur angulaire, angle autour du point) avec
 * `calculateCameraOptionsFromTo`. L'ancien survol réglait tout par le zoom et
 * des modes figés : impossible de dire « plus haut » ou « plus vite ».
 */
import maplibregl from 'maplibre-gl';
import { createGrip, makeDraggable } from './draggable.js';

const EARTH_RADIUS_M = 6371000;
const BASE_SPEED_MS = 100;               // ×1 = 100 m/s, une étape de 25 km en ~4 min
const SPEEDS = [0.5, 1, 2, 4, 8, 16];
const DEFAULT_SPEED = 4;                 // ~1 min par étape de 25 km
const DIST_MIN_M = 200;
const DIST_MAX_M = 8000;
const ELEV_MIN_DEG = 5;
const ELEV_MAX_DEG = 85;
const CLEARANCE_M = 30;                  // jamais plus près du sol
const HEADING_TAU_S = 1.5;               // lissage du cap
const TILE_WAIT_BUDGET_MS = 2000;        // au-delà, on avance même sans tuiles
const MAX_FRAME_DELTA_MS = 100;
const STAGE_PAUSE_MS = 3000;
const PROGRESS_REFRESH_MS = 100;
const STALL_RATE = 0.2;                  // allure pendant qu'on attend les tuiles
const RATE_TAU_S = 0.4;                  // ralentir et repartir sans à-coup
const ALT_TAU_S = 0.6;                   // lissage des altitudes lues sur le relief
const STOP_VIEW = { pitch: 45, zoom: 15 };

/** Vues de départ, réglables ensuite. dist en m, elev en degrés au-dessus de l'horizon, yaw autour du point. */
const PRESETS = [
  { id: 'derriere', label: 'Derrière', dist: 1500, elev: 30, yaw: 0 },
  { id: 'cote', label: 'Côté', dist: 1200, elev: 20, yaw: 90 },
  { id: 'rase', label: 'Rase-mottes', dist: 350, elev: 10, yaw: 0 },
  { id: 'haut', label: 'Haut', dist: 4000, elev: 45, yaw: 0 },
  { id: 'dessus', label: 'Dessus', dist: 3000, elev: 85, yaw: 0 }
];

let map = null;
let deps = null;

// Tracé
let route = [];        // [{lat, lon}]
let cumul = [];        // distance cumulée en m
let totalM = 0;
let elevations = null; // [m | null], alignées sur route
let stops = [];        // [{ m, title, detail }] fins d'étape
let stageSource = 'none';
let lastWaypoints = null; // rejoués quand le tracé arrive après eux

// Lecture
let open = false;
let playing = false;
let frameId = null;
let lastFrame = null;
let stalledMs = 0;
let posM = 0;
let speed = DEFAULT_SPEED;
let cam = { ...PRESETS[0] };
let heading = null;
let nextStop = 0;
let stageTimer = null;
let lastProgressRefresh = 0;
let rate = 1;                            // fraction de la vitesse réellement appliquée
let smoothTargetAlt = null;
let smoothCamAlt = null;
let profileCache = null;                 // { key, base, done } images du profil
let colors = null;                       // jetons CSS lus une fois par ouverture
let savedInteractions = null;

// DOM
let panel = null;
let card = null;
let head = null;
let ui = {};

// ── Géométrie ────────────────────────────────────────────────────────────

const rad = d => d * Math.PI / 180;
const deg = r => r * 180 / Math.PI;

function haversine(a, b) {
  const dLat = rad(b.lat - a.lat);
  const dLon = rad(b.lon - a.lon);
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(rad(a.lat)) * Math.cos(rad(b.lat)) * Math.sin(dLon / 2) ** 2;
  return 2 * EARTH_RADIUS_M * Math.asin(Math.sqrt(h));
}

function bearing(a, b) {
  const y = Math.sin(rad(b.lon - a.lon)) * Math.cos(rad(b.lat));
  const x = Math.cos(rad(a.lat)) * Math.sin(rad(b.lat))
    - Math.sin(rad(a.lat)) * Math.cos(rad(b.lat)) * Math.cos(rad(b.lon - a.lon));
  return (deg(Math.atan2(y, x)) + 360) % 360;
}

function destination(p, bearingDeg, distM) {
  const d = distM / EARTH_RADIUS_M;
  const b = rad(bearingDeg);
  const lat1 = rad(p.lat);
  const lat2 = Math.asin(Math.sin(lat1) * Math.cos(d) + Math.cos(lat1) * Math.sin(d) * Math.cos(b));
  const lon2 = rad(p.lon) + Math.atan2(Math.sin(b) * Math.sin(d) * Math.cos(lat1), Math.cos(d) - Math.sin(lat1) * Math.sin(lat2));
  return { lat: deg(lat2), lon: deg(lon2) };
}

/** Indice du segment contenant la distance m (recherche dichotomique). */
function segmentAt(m) {
  let lo = 0;
  let hi = cumul.length - 1;
  while (hi - lo > 1) {
    const mid = (lo + hi) >> 1;
    if (cumul[mid] <= m) lo = mid; else hi = mid;
  }
  return lo;
}

function pointAt(m) {
  if (m <= 0) return route[0];
  if (m >= totalM) return route[route.length - 1];
  const i = segmentAt(m);
  const t = (m - cumul[i]) / Math.max(cumul[i + 1] - cumul[i], 1e-6);
  return {
    lat: route[i].lat + (route[i + 1].lat - route[i].lat) * t,
    lon: route[i].lon + (route[i + 1].lon - route[i].lon) * t
  };
}

function elevationAt(m) {
  if (!elevations) return null;
  const i = segmentAt(Math.min(Math.max(m, 0), totalM));
  return elevations[i] ?? elevations[i + 1] ?? null;
}

function ground(p) {
  const e = map.queryTerrainElevation?.([p.lon, p.lat]);
  return Number.isFinite(e) ? e : 0;
}

function smoothAngle(prev, target, k) {
  if (prev === null) return target;
  const diff = ((target - prev + 540) % 360) - 180;
  return (prev + diff * k + 360) % 360;
}

/** Montée cumulée entre deux distances, depuis le profil. */
function ascentBetween(fromM, toM) {
  if (!elevations) return null;
  let up = 0;
  let prev = null;
  for (let i = segmentAt(fromM); i < route.length && cumul[i] <= toM; i++) {
    const e = elevations[i];
    if (e == null) continue;
    if (prev !== null && e > prev) up += e - prev;
    prev = e;
  }
  return up;
}

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
  route = Array.isArray(coords) ? coords.map(({ lat, lon }) => ({ lat, lon })) : [];
  cumul = [0];
  for (let i = 1; i < route.length; i++) cumul.push(cumul[i - 1] + haversine(route[i - 1], route[i]));
  totalM = cumul[cumul.length - 1] || 0;
  elevations = null;
  stops = [];
  stageSource = 'none';
  posM = 0;
  profileCache = null;
  dropProgressLayer();
  if (lastWaypoints) setFlyoverWaypoints(lastWaypoints);
}

/** Altitudes alignées sur le tracé ; null garde les précédentes. */
export function setFlyoverElevations(values) {
  if (Array.isArray(values) && values.length === route.length) {
    elevations = values;
    profileCache = null;
    drawProfile();
  }
}

/** Fins d'étape du planificateur : [{ endKm, title, detail }]. null les retire. */
export function setFlyoverStages(stages) {
  if (!stages) {
    if (stageSource === 'plan') { stops = []; stageSource = 'none'; }
    profileCache = null;
    drawProfile();
    return;
  }
  stops = stages
    .map(s => ({ m: s.endKm * 1000, title: s.title, detail: s.detail }))
    .filter(s => s.m > 0 && s.m < totalM - 50);
  stageSource = 'plan';
  resetNextStop();
  profileCache = null;
  drawProfile();
}

/** Points de passage : étapes par défaut tant que le planificateur n'a pas tourné. */
export function setFlyoverWaypoints(waypoints) {
  lastWaypoints = Array.isArray(waypoints) ? waypoints : null;
  if (stageSource === 'plan' || route.length < 2 || !Array.isArray(waypoints)) return;
  const inner = waypoints.slice(1, -1);
  stops = inner.map((w, k) => {
    let best = 0;
    let bestD = Infinity;
    for (let i = 0; i < route.length; i++) {
      const d = haversine(w, route[i]);
      if (d < bestD) { bestD = d; best = i; }
    }
    return { m: cumul[best], title: `Point ${k + 2}`, detail: null };
  }).filter(s => s.m > 0 && s.m < totalM - 50).sort((a, b) => a.m - b.m);
  stageSource = stops.length ? 'waypoints' : 'none';
  resetNextStop();
  profileCache = null;
  drawProfile();
}

export function isFlyoverOpen() {
  return open;
}

/** Ouvre le panneau et lance la lecture. */
export function startFlyover() {
  if (route.length < 2 || totalM < 10) {
    console.warn('[flyover] pas de tracé à survoler');
    return;
  }
  if (!open) openPanel();
  play();
}

/** Ferme le panneau et revient à la vue 3D posée. */
export function stopFlyover() {
  if (!open) return;
  pause();
  open = false;
  hideCard();
  panel?.remove();
  panel = null;
  head?.remove();
  head = null;
  setProgress(null);
  restoreInteractions();
  map?.easeTo({ pitch: STOP_VIEW.pitch, zoom: STOP_VIEW.zoom, roll: 0, duration: 1200 });
  console.debug('[flyover] fermé');
}

/** Bouton de la barre de contrôles : ouvre/ferme le survol. */
export function createFlyoverControl() {
  class FlyoverControl {
    onAdd() {
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';
      const btn = document.createElement('button');
      btn.className = 'maplibregl-ctrl-animation';
      btn.textContent = '▶';
      btn.title = 'Survol 3D du tracé';
      btn.onclick = () => (open ? stopFlyover() : startFlyover());
      this._container.appendChild(btn);
      return this._container;
    }

    onRemove() {
      this._container.remove();
    }
  }
  return new FlyoverControl();
}

// ── Lecture ──────────────────────────────────────────────────────────────

function resetNextStop() {
  nextStop = stops.findIndex(s => s.m > posM + 1);
  if (nextStop < 0) nextStop = stops.length;
}

function play() {
  if (playing) return;
  if (posM >= totalM - 1) seek(0);
  deps.enableTerrain();
  hideCard();
  playing = true;
  lastFrame = null;
  stalledMs = 0;
  rate = 1;
  frameId = requestAnimationFrame(frame);
  updatePanel();
  console.debug('[flyover] lecture', `${(totalM / 1000).toFixed(1)} km`, `×${speed}`, cam.id);
}

function pause() {
  playing = false;
  if (frameId !== null) cancelAnimationFrame(frameId);
  frameId = null;
  clearTimeout(stageTimer);
  stageTimer = null;
  updatePanel();
}

function togglePlay() {
  if (playing || stageTimer) pause(); else play();
}

function seek(m) {
  posM = Math.min(Math.max(m, 0), totalM);
  heading = null;
  smoothTargetAlt = null;
  smoothCamAlt = null;
  resetNextStop();
  hideCard();
  render(true);
}

function frame(ts) {
  const delta = lastFrame === null ? 0 : Math.min(ts - lastFrame, MAX_FRAME_DELTA_MS);
  lastFrame = ts;

  // Tuiles en retard : on ralentit en douceur au lieu de s'arrêter net, sinon
  // la caméra avance par à-coups ; au-delà du budget, on repart quand même.
  const tilesReady = typeof map.areTilesLoaded !== 'function' || map.areTilesLoaded();
  stalledMs = tilesReady ? 0 : stalledMs + delta;
  const wantedRate = tilesReady || stalledMs > TILE_WAIT_BUDGET_MS ? 1 : STALL_RATE;
  rate += (wantedRate - rate) * (1 - Math.exp(-delta / 1000 / RATE_TAU_S));
  posM += BASE_SPEED_MS * speed * rate * delta / 1000;

  if (nextStop < stops.length && posM >= stops[nextStop].m) {
    posM = stops[nextStop].m;
    render(true);
    stageBreak(stops[nextStop]);
    nextStop++;
    return;
  }
  if (posM >= totalM) {
    posM = totalM;
    render(true);
    pause();
    showCard({ title: 'Arrivée', detail: null });
    return;
  }

  render(false, delta / 1000);
  frameId = requestAnimationFrame(frame);
}

/** Pause à une fin d'étape, carte affichée, puis reprise sauf pause manuelle. */
function stageBreak(stop) {
  playing = false;
  frameId = null;
  showCard(stop);
  updatePanel();
  stageTimer = setTimeout(() => {
    stageTimer = null;
    play();
  }, STAGE_PAUSE_MS);
}

// ── Caméra ───────────────────────────────────────────────────────────────

function render(snap, dt = 0) {
  if (!map || route.length < 2) return;
  const target = pointAt(posM);
  const look = Math.min(Math.max(cam.dist * 1.5, 500), 3000);
  const ahead = pointAt(Math.min(posM + look, totalM));
  const wanted = haversine(target, ahead) > 1 ? bearing(target, ahead) : (heading ?? 0);
  heading = snap ? wanted : smoothAngle(heading, wanted, 1 - Math.exp(-dt / HEADING_TAU_S));

  // Le relief s'affine quand ses tuiles arrivent : lu brut, il fait tressauter
  // la caméra. On lisse les deux altitudes.
  const k = snap ? 1 : 1 - Math.exp(-dt / ALT_TAU_S);
  smoothTargetAlt = smoothTargetAlt === null ? ground(target) : smoothTargetAlt + (ground(target) - smoothTargetAlt) * k;
  const targetAlt = smoothTargetAlt;
  const horiz = cam.dist * Math.cos(rad(cam.elev));
  const camPos = destination(target, heading + 180 + cam.yaw, Math.max(horiz, 1));
  const wantedCamAlt = Math.max(targetAlt + cam.dist * Math.sin(rad(cam.elev)), ground(camPos) + CLEARANCE_M);
  smoothCamAlt = smoothCamAlt === null || snap ? wantedCamAlt : smoothCamAlt + (wantedCamAlt - smoothCamAlt) * k;
  const camAlt = Math.max(smoothCamAlt, targetAlt + 5);

  try {
    const opts = map.calculateCameraOptionsFromTo(
      [camPos.lon, camPos.lat], camAlt, [target.lon, target.lat], targetAlt
    );
    map.jumpTo(opts);
  } catch (e) {
    // Caméra et cible confondues : rien à faire pour cette image.
  }

  if (!head) {
    const el = document.createElement('div');
    el.className = 'flyover-head';
    head = new maplibregl.Marker({ element: el }).setLngLat([target.lon, target.lat]).addTo(map);
  } else {
    head.setLngLat([target.lon, target.lat]);
  }

  const now = performance.now();
  if (snap || now - lastProgressRefresh > PROGRESS_REFRESH_MS) {
    lastProgressRefresh = now;
    setProgress(posM / totalM);
    updatePanel();
  }
}

/**
 * Tracé parcouru : une source fixe (tout le tracé) et un dégradé en marche
 * d'escalier sur line-progress. Remplacer la géométrie à chaque rafraîchissement
 * faisait recalculer la source par MapLibre, d'où une image perdue.
 */
function setProgress(fraction) {
  if (!map) return;
  const layer = map.getLayer('flyover-progress');
  if (fraction === null) {
    if (layer) map.setLayoutProperty('flyover-progress', 'visibility', 'none');
    return;
  }
  if (!layer) {
    if (!map.isStyleLoaded()) return;
    const data = {
      type: 'Feature',
      properties: {},
      geometry: { type: 'LineString', coordinates: route.map(p => [p.lon, p.lat]) }
    };
    const src = map.getSource('flyover-progress');
    if (src) src.setData(data);
    else map.addSource('flyover-progress', { type: 'geojson', data, lineMetrics: true });
    map.addLayer({
      id: 'flyover-progress',
      type: 'line',
      source: 'flyover-progress',
      layout: { 'line-cap': 'round', 'line-join': 'round' },
      paint: { 'line-width': 6, 'line-opacity': 0.85, 'line-gradient': progressGradient(0) }
    });
    return;
  }
  map.setLayoutProperty('flyover-progress', 'visibility', 'visible');
  map.setPaintProperty('flyover-progress', 'line-gradient', progressGradient(fraction));
}

function progressGradient(fraction) {
  const f = Math.min(Math.max(fraction, 1e-6), 1);
  return ['step', ['line-progress'], colors.done, f, 'rgba(0, 0, 0, 0)'];
}

/** Le tracé a changé : la source du tracé parcouru est à refaire. */
function dropProgressLayer() {
  if (map?.getLayer('flyover-progress')) map.removeLayer('flyover-progress');
  if (map?.getSource('flyover-progress')) map.removeSource('flyover-progress');
}

// ── Interactions pendant le survol ───────────────────────────────────────

function takeInteractions() {
  if (savedInteractions) return;
  savedInteractions = {
    scrollZoom: map.scrollZoom.isEnabled(),
    dragRotate: map.dragRotate.isEnabled(),
    keyboard: map.keyboard.isEnabled(),
    maxPitch: map.getMaxPitch()
  };
  map.scrollZoom.disable();
  map.dragRotate.disable();
  map.keyboard.disable();
  map.setMaxPitch(85);
  const canvas = map.getCanvasContainer();
  canvas.addEventListener('wheel', onWheel, { passive: false });
  canvas.addEventListener('mousedown', onRotateStart);
  canvas.addEventListener('contextmenu', preventDefault);
  window.addEventListener('keydown', onKey);
}

function restoreInteractions() {
  if (!savedInteractions || !map) return;
  const s = savedInteractions;
  savedInteractions = null;
  if (s.scrollZoom) map.scrollZoom.enable();
  if (s.dragRotate) map.dragRotate.enable();
  if (s.keyboard) map.keyboard.enable();
  map.setMaxPitch(s.maxPitch);
  const canvas = map.getCanvasContainer();
  canvas.removeEventListener('wheel', onWheel);
  canvas.removeEventListener('mousedown', onRotateStart);
  canvas.removeEventListener('contextmenu', preventDefault);
  window.removeEventListener('keydown', onKey);
}

function preventDefault(e) {
  e.preventDefault();
}

function setCam(changes) {
  cam = {
    ...cam,
    ...changes,
    dist: Math.min(Math.max(changes.dist ?? cam.dist, DIST_MIN_M), DIST_MAX_M),
    elev: Math.min(Math.max(changes.elev ?? cam.elev, ELEV_MIN_DEG), ELEV_MAX_DEG)
  };
  if (!playing) render(true);
  updatePanel();
}

function onWheel(e) {
  e.preventDefault();
  setCam({ dist: cam.dist * Math.exp(e.deltaY * 0.0015), id: 'libre' });
}

function onRotateStart(e) {
  if (e.button !== 2) return;
  let x = e.clientX;
  let y = e.clientY;
  const move = ev => {
    setCam({ yaw: cam.yaw + (ev.clientX - x) * 0.3, elev: cam.elev + (ev.clientY - y) * 0.2, id: 'libre' });
    x = ev.clientX;
    y = ev.clientY;
  };
  const up = () => {
    window.removeEventListener('mousemove', move);
    window.removeEventListener('mouseup', up);
  };
  window.addEventListener('mousemove', move);
  window.addEventListener('mouseup', up);
}

function onKey(e) {
  if (e.target.closest?.('input, textarea, select, [contenteditable]')) return;
  const actions = {
    ' ': togglePlay,
    ArrowRight: () => seek(posM + 1000),
    ArrowLeft: () => seek(posM - 1000),
    ArrowUp: () => setCam({ elev: cam.elev + 5, id: 'libre' }),
    ArrowDown: () => setCam({ elev: cam.elev - 5, id: 'libre' }),
    '+': () => setSpeed(1),
    '=': () => setSpeed(1),
    '-': () => setSpeed(-1),
    Escape: stopFlyover
  };
  const action = actions[e.key];
  if (!action) return;
  e.preventDefault();
  action();
}

function setSpeed(step) {
  const i = SPEEDS.indexOf(speed);
  speed = SPEEDS[Math.min(Math.max(i + step, 0), SPEEDS.length - 1)];
  updatePanel();
}

// ── Panneau ──────────────────────────────────────────────────────────────

function el(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
}

function openPanel() {
  open = true;
  colors = {
    done: deps.mapColor('--map-flyover-done'),
    profile: deps.mapColor('--map-flyover-profile'),
    stop: deps.mapColor('--map-flyover-stop'),
    cursor: deps.mapColor('--map-flyover-cursor')
  };
  smoothTargetAlt = null;
  smoothCamAlt = null;
  posM = 0;
  heading = null;
  resetNextStop();
  takeInteractions();

  panel = el('div', 'flyover-panel');
  panel.setAttribute('role', 'region');
  panel.setAttribute('aria-label', 'Survol 3D');

  const row = el('div', 'flyover-row');
  ui.play = el('button', 'flyover-play');
  ui.play.type = 'button';
  ui.play.onclick = togglePlay;
  ui.stats = el('span', 'flyover-stats t-metric');
  const close = el('button', 'flyover-close', '✕');
  close.type = 'button';
  close.title = 'Fermer le survol (Échap)';
  close.onclick = stopFlyover;

  const slower = el('button', 'flyover-speed-btn', '−');
  slower.type = 'button';
  slower.title = 'Plus lent (−)';
  slower.onclick = () => setSpeed(-1);
  ui.speed = el('span', 'flyover-speed t-metric');
  const faster = el('button', 'flyover-speed-btn', '+');
  faster.type = 'button';
  faster.title = 'Plus vite (+)';
  faster.onclick = () => setSpeed(1);

  const grip = createGrip();
  row.append(grip, ui.play, slower, ui.speed, faster, ui.stats, close);

  ui.canvas = el('canvas', 'flyover-profile');
  ui.canvas.title = 'Cliquer ou glisser pour se déplacer';
  ui.canvas.addEventListener('pointerdown', onProfilePointer);

  const presets = el('div', 'flyover-presets');
  ui.presets = PRESETS.map(p => {
    const b = el('button', 'flyover-preset', p.label);
    b.type = 'button';
    b.onclick = () => setCam({ ...p });
    presets.append(b);
    return [p.id, b];
  });

  const sliders = el('div', 'flyover-sliders');
  ui.dist = slider('Distance', 0, 1000, v => setCam({ dist: DIST_MIN_M * (DIST_MAX_M / DIST_MIN_M) ** (v / 1000), id: 'libre' }));
  ui.elev = slider('Hauteur', ELEV_MIN_DEG, ELEV_MAX_DEG, v => setCam({ elev: v, id: 'libre' }));
  sliders.append(ui.dist.wrap, ui.elev.wrap);

  const hint = el('p', 'flyover-hint', 'Molette : distance · clic droit glissé : tourner · ← → : 1 km · espace : pause');

  panel.append(row, ui.canvas, presets, sliders, hint);
  map.getContainer().append(panel);
  makeDraggable(panel, grip, 'cn.drag.flyover');
  drawProfile();
  updatePanel();
}

function slider(label, min, max, onInput) {
  const wrap = el('label', 'flyover-slider');
  const span = el('span', null, label);
  const input = el('input');
  input.type = 'range';
  input.min = min;
  input.max = max;
  input.addEventListener('input', () => onInput(Number(input.value)));
  wrap.append(span, input);
  return { wrap, input };
}

function onProfilePointer(e) {
  const canvas = ui.canvas;
  canvas.setPointerCapture(e.pointerId);
  const toM = ev => {
    const r = canvas.getBoundingClientRect();
    return Math.min(Math.max((ev.clientX - r.left) / r.width, 0), 1) * totalM;
  };
  seek(toM(e));
  const move = ev => seek(toM(ev));
  const up = () => {
    canvas.removeEventListener('pointermove', move);
    canvas.removeEventListener('pointerup', up);
  };
  canvas.addEventListener('pointermove', move);
  canvas.addEventListener('pointerup', up);
}

function updatePanel() {
  if (!panel) return;
  ui.play.textContent = playing || stageTimer ? '⏸' : '▶';
  ui.play.title = playing || stageTimer ? 'Pause (espace)' : 'Lecture (espace)';
  ui.speed.textContent = `×${speed}`;
  const e = elevationAt(posM);
  const slope = elevations ? slopeAt(posM) : null;
  ui.stats.textContent = [
    `km ${(posM / 1000).toFixed(1)} / ${(totalM / 1000).toFixed(1)}`,
    e != null ? `${Math.round(e)} m` : null,
    slope != null ? `${slope > 0 ? '+' : ''}${slope.toFixed(0)} %` : null
  ].filter(Boolean).join(' · ');
  ui.presets.forEach(([id, b]) => b.classList.toggle('active', id === cam.id));
  ui.dist.input.value = Math.round(1000 * Math.log(cam.dist / DIST_MIN_M) / Math.log(DIST_MAX_M / DIST_MIN_M));
  ui.elev.input.value = Math.round(cam.elev);
  drawProfile();
}

/** Pente moyenne sur 200 m autour du point, en %. */
function slopeAt(m) {
  const a = elevationAt(Math.max(m - 100, 0));
  const b = elevationAt(Math.min(m + 100, totalM));
  const span = Math.min(m + 100, totalM) - Math.max(m - 100, 0);
  return a != null && b != null && span > 0 ? (b - a) / span * 100 : null;
}

/** Profil fixe (fond, partie parcourue, étapes) dessiné une fois, puis découpé au curseur. */
function profileImages(w, h, dpr) {
  const key = `${w}x${h}x${dpr}x${stops.length}`;
  if (profileCache?.key === key) return profileCache;
  const x = m => m / totalM * w;
  const known = elevations?.filter(v => v != null) ?? [];
  let lo = Infinity;
  let hi = -Infinity;
  known.forEach(v => { if (v < lo) lo = v; if (v > hi) hi = v; });
  if (!known.length) { lo = 0; hi = 1; }
  const y = v => h - 4 - (v - lo) / Math.max(hi - lo, 1) * (h - 10);

  const paint = color => {
    const c = document.createElement('canvas');
    c.width = w * dpr;
    c.height = h * dpr;
    const ctx = c.getContext('2d');
    ctx.scale(dpr, dpr);
    ctx.beginPath();
    ctx.moveTo(0, h);
    if (known.length) {
      route.forEach((_, i) => {
        if (elevations[i] != null) ctx.lineTo(x(cumul[i]), y(elevations[i]));
      });
    } else {
      ctx.lineTo(0, h / 2);
      ctx.lineTo(w, h / 2);
    }
    ctx.lineTo(w, h);
    ctx.closePath();
    ctx.fillStyle = color;
    ctx.fill();
    ctx.fillStyle = colors.stop;
    stops.forEach(s => ctx.fillRect(Math.round(x(s.m)) - 1, 0, 2, h));
    return c;
  };
  profileCache = { key, base: paint(colors.profile), done: paint(colors.done) };
  return profileCache;
}

function drawProfile() {
  const canvas = ui.canvas;
  if (!panel || !canvas || totalM <= 0 || !colors) return;
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (!w || !h) return;
  if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
    canvas.width = w * dpr;
    canvas.height = h * dpr;
  }
  const { base, done } = profileImages(w, h, dpr);
  const ctx = canvas.getContext('2d');
  const cut = Math.round(posM / totalM * w * dpr);
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(base, 0, 0);
  if (cut > 0) ctx.drawImage(done, 0, 0, cut, canvas.height, 0, 0, cut, canvas.height);
  ctx.fillStyle = colors.cursor;
  ctx.fillRect(cut - dpr, 0, 2 * dpr, canvas.height);
}

// ── Carte d'étape ────────────────────────────────────────────────────────

function showCard(stop) {
  hideCard();
  const prev = stops.filter(s => s.m < posM - 1).pop();
  const fromM = prev ? prev.m : 0;
  const up = ascentBetween(fromM, posM);
  card = el('div', 'flyover-card');
  card.append(el('strong', null, stop.title));
  const facts = [
    `${((posM - fromM) / 1000).toFixed(1)} km`,
    up != null ? `D+ ${Math.round(up)} m` : null,
    stop.detail
  ].filter(Boolean).join(' · ');
  card.append(el('span', null, facts));
  map.getContainer().append(card);
}

function hideCard() {
  card?.remove();
  card = null;
}
