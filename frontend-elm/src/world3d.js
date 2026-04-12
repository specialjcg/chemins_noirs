/**
 * world3d.js — Three.js renderer for the Course d'Orientation game.
 *
 * Architecture:
 * - Elm = source of truth (player position, control points, scene data)
 * - This module = pure renderer, mounts a canvas and updates meshes from
 *   ports payloads. No state of its own besides the Three.js scene.
 *
 * Public API (called by main.js port subscribers):
 *   init({ lat, lon, bearing })       -- mount canvas, start render loop
 *   setTerrain(elevationGrid)         -- (re)build heightmap mesh
 *   setRoads(roads)                   -- (re)build road meshes
 *   setVegetation(zones)              -- (re)build vegetation instances
 *   setBuildings(buildings)           -- (re)build building meshes
 *   setControlPoints(cps)             -- (re)build CP markers
 *   updateCamera({ lat, lon, bearing })  -- per-frame camera update
 *   destroy()                          -- tear down everything
 *
 * Coordinate system:
 *   X = east (meters from terrain center)
 *   Y = up   (meters above sea level)
 *   Z = south (meters from terrain center)
 *   This matches Three.js Y-up convention.
 */

import * as THREE from 'three';
import {
  EARTH_RADIUS,
  latLonToXZ as geomLatLonToXZ,
  xzToLatLon as geomXzToLatLon,
  sampleAlt as geomSampleAlt,
  sampleAltTriangulated as geomSampleAltTriangulated,
  planeJToDemRow,
} from './world3d_geom.js';
const EYE_HEIGHT = 1.7;       // meters above terrain
const VIEW_DISTANCE = 1500;   // far clip
const FOV = 75;

// ============================================================
// State (single renderer instance — Course d'Orientation is a singleton)
// ============================================================

let renderer = null;
let scene = null;
let camera = null;
let container = null;
let rafHandle = null;
let resizeObserver = null;

// Origin of the local meter coordinate system (set on init from player start)
let origin = { lat: 0, lon: 0 };

// Latest player state (driven by updateCamera)
let player = { lat: 0, lon: 0, bearing: 0 };

// Latest terrain (kept so we can sample altitude when (re)building roads/veg/buildings)
let terrain = null;  // { gridArray, originLat, originLon, cellSizeM, rows, cols, minAlt, mesh }

// Mesh groups for cleanup on update
let terrainMesh = null;
let roadGroup = null;
let vegetationGroup = null;
let buildingGroup = null;
let controlPointGroup = null;
let playerMarker = null; // orange ring on ground at player position (debug)

// Cached raw payloads — replayed when terrain arrives (or when init runs after them)
// because road/veg/building/CP altitudes depend on the terrain heightmap.
// Elm port commands are NOT order-guaranteed — payloads may arrive before init().
let lastTerrainPayload = null;
let lastRoadsPayload = null;
let lastVegetationPayload = null;
let lastBuildingsPayload = null;
let lastControlPointsPayload = null;

// ============================================================
// Coordinate helpers
// ============================================================

/** Convert lat/lon to local scene meters relative to `origin`. Thin wrapper over the
 * tested pure function world3d_geom.latLonToXZ. */
function latLonToXZ(lat, lon) {
  return geomLatLonToXZ(origin, lat, lon);
}

/** Bilinear elevation grid sample at a lat/lon. Uses the tested pure function. */
function sampleAlt(lat, lon) {
  return geomSampleAlt(terrain, lat, lon);
}

/** Triangulated elevation sample — matches EXACTLY the visible terrain mesh
 * (Three.js PlaneGeometry with the same SW→NE diagonal). Use this for placing
 * roads/buildings/vegetation/CPs to guarantee zero mismatch with what the user sees.
 * Pure-math, O(1) per call (no raycasting). */
function sampleAltTri(lat, lon) {
  return geomSampleAltTriangulated(terrain, lat, lon);
}

// Reusable raycaster + temp vectors to avoid GC pressure during mesh construction
const _altRaycaster = new THREE.Raycaster();
const _altRayOrigin = new THREE.Vector3();
const _altDownDir = new THREE.Vector3(0, -1, 0);

/** Sample the EXACT rendered terrain surface altitude at scene (X, Z) by raycasting against
 * the terrain mesh. This is the source of truth for placing roads/trees/buildings/CPs and
 * guarantees zero mismatch with what the user sees. */
function surfaceAltAt(x, z) {
  if (!terrainMesh) return 0;
  const high = (terrain ? terrain.maxAlt : 1000) + 200;
  _altRayOrigin.set(x, high, z);
  _altRaycaster.set(_altRayOrigin, _altDownDir);
  const hits = _altRaycaster.intersectObject(terrainMesh, false);
  if (hits.length > 0) return hits[0].point.y;
  // Fallback to bilinear if ray misses
  if (terrain) {
    const ll = geomXzToLatLon(origin, x, z);
    return sampleAlt(ll.lat, ll.lon);
  }
  return 0;
}

/** Convenience: surfaceAltAt by lat/lon. */
function surfaceAlt(lat, lon) {
  const { x, z } = latLonToXZ(lat, lon);
  return surfaceAltAt(x, z);
}

// ============================================================
// Lifecycle
// ============================================================

export function init({ lat, lon, bearing }) {
  console.log('[world3d] init', lat, lon, bearing);

  origin = { lat, lon };
  player = { lat, lon, bearing };

  // Mount container — Elm renders <div id="world3d-root">
  container = document.getElementById('world3d-root');
  if (!container) {
    console.error('[world3d] #world3d-root not found in DOM');
    return;
  }

  // Clear any previous canvas
  while (container.firstChild) container.removeChild(container.firstChild);

  const width = container.clientWidth || window.innerWidth;
  const height = container.clientHeight || window.innerHeight;

  scene = new THREE.Scene();
  scene.background = new THREE.Color(0x87ceeb); // sky blue
  scene.fog = new THREE.Fog(0x87ceeb, VIEW_DISTANCE * 0.5, VIEW_DISTANCE);

  camera = new THREE.PerspectiveCamera(FOV, width / height, 0.5, VIEW_DISTANCE * 1.2);

  renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.setSize(width, height);
  renderer.shadowMap.enabled = false;
  container.appendChild(renderer.domElement);

  // Lighting: sun + ambient
  const sun = new THREE.DirectionalLight(0xfff5e0, 1.4);
  sun.position.set(300, 500, 200);
  scene.add(sun);
  scene.add(new THREE.AmbientLight(0xffffff, 0.55));

  // Fallback flat green ground (replaced when terrain arrives)
  const flatGround = new THREE.Mesh(
    new THREE.PlaneGeometry(2000, 2000),
    new THREE.MeshLambertMaterial({ color: 0x4b8a2d })
  );
  flatGround.rotation.x = -Math.PI / 2;
  flatGround.position.y = 0;
  flatGround.name = 'fallback-ground';
  scene.add(flatGround);


  // Resize handling
  resizeObserver = new ResizeObserver(() => {
    if (!renderer || !camera || !container) return;
    const w = container.clientWidth || window.innerWidth;
    const h = container.clientHeight || window.innerHeight;
    renderer.setSize(w, h);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  });
  resizeObserver.observe(container);

  updateCameraTransform();
  startRenderLoop();

  // Replay any payloads that arrived before init() (Elm port order is not guaranteed).
  // Terrain MUST be replayed first so dependent items (roads/veg/buildings/CPs) get
  // correct altitudes from sampleAlt().
  if (lastTerrainPayload) {
    console.log('[world3d] init: replaying cached terrain');
    applyTerrain(lastTerrainPayload);
  }
  if (lastRoadsPayload) {
    console.log('[world3d] init: replaying cached roads');
    applyRoads(lastRoadsPayload);
  }
  if (lastVegetationPayload) {
    console.log('[world3d] init: replaying cached vegetation');
    applyVegetation(lastVegetationPayload);
  }
  if (lastBuildingsPayload) {
    console.log('[world3d] init: replaying cached buildings');
    applyBuildings(lastBuildingsPayload);
  }
  if (lastControlPointsPayload) {
    console.log('[world3d] init: replaying cached control points');
    applyControlPoints(lastControlPointsPayload);
  }
}

export function destroy() {
  console.log('[world3d] destroy');
  stopRenderLoop();

  if (resizeObserver) {
    resizeObserver.disconnect();
    resizeObserver = null;
  }

  if (scene) {
    scene.traverse((obj) => {
      if (obj.geometry) obj.geometry.dispose();
      if (obj.material) {
        if (Array.isArray(obj.material)) obj.material.forEach((m) => m.dispose());
        else obj.material.dispose();
      }
    });
    scene = null;
  }

  if (renderer) {
    renderer.dispose();
    if (renderer.domElement && renderer.domElement.parentNode) {
      renderer.domElement.parentNode.removeChild(renderer.domElement);
    }
    renderer = null;
  }

  camera = null;
  container = null;
  terrain = null;
  terrainMesh = null;
  roadGroup = null;
  vegetationGroup = null;
  buildingGroup = null;
  controlPointGroup = null;
  playerMarker = null;

  // Clear cached payloads — next game starts fresh
  lastTerrainPayload = null;
  lastRoadsPayload = null;
  lastVegetationPayload = null;
  lastBuildingsPayload = null;
  lastControlPointsPayload = null;
}

function startRenderLoop() {
  const loop = () => {
    if (!renderer) return;
    rafHandle = requestAnimationFrame(loop);
    renderer.render(scene, camera);
  };
  loop();
}

function stopRenderLoop() {
  if (rafHandle !== null) {
    cancelAnimationFrame(rafHandle);
    rafHandle = null;
  }
}

// ============================================================
// Camera
// ============================================================

export function updateCamera({ lat, lon, bearing }) {
  player = { lat, lon, bearing };
  if (!camera) return;
  updateCameraTransform();
}

function updateCameraTransform() {
  if (!camera) return;
  const { x, z } = latLonToXZ(player.lat, player.lon);
  // Use raycast against terrain mesh for exact eye level.
  // Falls back to bilinear sampleAlt if mesh not yet ready.
  const groundY = terrainMesh ? surfaceAltAt(x, z) : sampleAlt(player.lat, player.lon);
  const eyeY = groundY + EYE_HEIGHT;

  camera.position.set(x, eyeY, z);

  // Bearing in degrees, clockwise from north (compass).
  // Convert to a look-at direction in scene XZ.
  // North = -Z, East = +X. So:
  //   dx = sin(bearing), dz = -cos(bearing)
  const rad = player.bearing * Math.PI / 180;
  const dx = Math.sin(rad);
  const dz = -Math.cos(rad);
  const target = new THREE.Vector3(x + dx * 30, eyeY - 0.5, z + dz * 30);
  camera.lookAt(target);
}

// ============================================================
// Terrain
// ============================================================

export function setTerrain(grid) {
  console.log('[world3d] setTerrain', grid.rows, 'x', grid.cols, 'cell=', grid.cellSizeM, 'm');
  lastTerrainPayload = grid;
  if (!scene) {
    console.log('[world3d] setTerrain deferred (scene not ready)');
    return;
  }
  applyTerrain(grid);

  // Replay all dependent payloads so they get correct altitudes from sampleAlt().
  if (lastRoadsPayload) applyRoads(lastRoadsPayload);
  if (lastVegetationPayload) applyVegetation(lastVegetationPayload);
  if (lastBuildingsPayload) applyBuildings(lastBuildingsPayload);
  if (lastControlPointsPayload) applyControlPoints(lastControlPointsPayload);
}

function applyTerrain(grid) {
  // Convert nested list to flat 2D array for fast access
  const gridArray = grid.grid; // [[Float]]
  terrain = {
    gridArray,
    originLat: grid.originLat,
    originLon: grid.originLon,
    cellSizeM: grid.cellSizeM,
    rows: grid.rows,
    cols: grid.cols,
    minAlt: grid.minAlt,
    maxAlt: grid.maxAlt,
  };

  // Remove fallback ground if still present
  const fallback = scene.getObjectByName('fallback-ground');
  if (fallback) {
    scene.remove(fallback);
    fallback.geometry.dispose();
    fallback.material.dispose();
  }
  // Remove previous terrain mesh
  if (terrainMesh) {
    scene.remove(terrainMesh);
    terrainMesh.geometry.dispose();
    terrainMesh.material.dispose();
    terrainMesh = null;
  }

  // Build a PlaneGeometry sized to the grid extent and displace each vertex.
  const widthM = grid.cellSizeM * (grid.cols - 1);
  const heightM = grid.cellSizeM * (grid.rows - 1);
  const geometry = new THREE.PlaneGeometry(widthM, heightM, grid.cols - 1, grid.rows - 1);
  geometry.rotateX(-Math.PI / 2); // make plane horizontal (Y-up)

  // North-south flip — see world3d_geom.planeJToDemRow.
  // PlaneGeometry vertex (i, j=0) is at scene -Z (NORTH after rotateX(-PI/2)),
  // but the DEM has grid[0] = SOUTH row. The flip is tested in world3d_geom.test.mjs.
  const positions = geometry.attributes.position;
  for (let j = 0; j < grid.rows; j++) {
    const demRow = planeJToDemRow(j, grid.rows);
    for (let i = 0; i < grid.cols; i++) {
      const idx = j * grid.cols + i;
      positions.setY(idx, gridArray[demRow][i]);
    }
  }
  positions.needsUpdate = true;
  geometry.computeVertexNormals();
  // CRITICAL for raycaster: bounding volumes must be recomputed after vertex modification,
  // otherwise Raycaster's early-out test against the bounding sphere may discard the mesh.
  geometry.computeBoundingBox();
  geometry.computeBoundingSphere();

  // Position the plane so its origin matches the grid origin in scene coords.
  // PlaneGeometry is centered at (0,0,0). Shift so vertex (0,0) lands at the grid's
  // (originLat, originLon) projected into scene meters.
  const originXZ = latLonToXZ(grid.originLat, grid.originLon);
  // After rotation, the plane's center is at (0, 0, 0). Vertex (0,0) is at (-w/2, 0, +h/2) in local.
  // We want vertex (0,0) at (originXZ.x, ?, originXZ.z + heightM)? Actually grid (0,0) is at originLat, originLon.
  // In scene coords, that's (originXZ.x, _, originXZ.z). The plane vertex (0,0) post-rotation is at
  // (-w/2, 0, +h/2) relative to the plane's center. So plane center should be at (originXZ.x + w/2, _, originXZ.z - h/2).
  // BUT wait — rotated PlaneGeometry's vertex j=0 i=0 is actually at (-w/2, 0, -h/2) due to the rotation conventions.
  // Let me just set position to align center to grid center: grid spans rows×cellSize south-to-north,
  // so grid center is (originLat + (rows/2)*cellM, originLon + (cols/2)*cellM_lon).
  // Simpler: compute the grid center directly and place the plane there.
  const centerLat = grid.originLat + (heightM / 2) / 111000;
  const centerLon = grid.originLon + (widthM / 2) / (111000 * Math.cos(grid.originLat * Math.PI / 180));
  const centerXZ = latLonToXZ(centerLat, centerLon);

  // Initialize per-vertex color buffer with the base "pâture" tone + per-vertex jitter
  // (small RGB noise + slight altitude-driven brown tint at high ground).
  // recolorTerrainFromVegetation() will later overwrite forest/vine vertices.
  const vertexCount = grid.rows * grid.cols;
  const colors = new Float32Array(vertexCount * 3);
  for (let j = 0; j < grid.rows; j++) {
    const demRow = planeJToDemRow(j, grid.rows);
    for (let i = 0; i < grid.cols; i++) {
      const idx = j * grid.cols + i;
      const alt = gridArray[demRow][i];
      const c = jitterColor(TERRAIN_BASE_COLOR, i, j, alt, grid.minAlt, grid.maxAlt);
      colors[idx * 3]     = c.r;
      colors[idx * 3 + 1] = c.g;
      colors[idx * 3 + 2] = c.b;
    }
  }
  geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));

  const material = new THREE.MeshLambertMaterial({
    vertexColors: true,
    flatShading: false,
  });
  terrainMesh = new THREE.Mesh(geometry, material);
  terrainMesh.position.set(centerXZ.x, 0, centerXZ.z);
  terrainMesh.name = 'terrain';
  scene.add(terrainMesh);

  // Recompute camera Y now that we have terrain altitudes
  updateCameraTransform();

  // If vegetation already arrived, recolor immediately. Otherwise applyVegetation
  // will trigger the recolor when it runs.
  if (lastVegetationPayload) {
    recolorTerrainFromVegetation(lastVegetationPayload);
  }

  // Re-place existing roads/veg/buildings/CPs since their altitudes depend on terrain
  // (they were rendered before terrain arrived → at sea level)
  // We don't store the raw payloads, so caller must re-send. In practice the order is:
  //   StartGame → enterGameView → fetch terrain (arrives first usually) → fetch roads/veg/buildings
  // Even if order is reversed, the next setRoads/etc call rebuilds at correct altitude.
}

// Default terrain color = pâture/prairie du Beaujolais — vrai vert d'herbe.
// Plus naturaliste que la convention "yellow open land" des cartes CO papier.
const TERRAIN_BASE_COLOR = { r: 0.42, g: 0.62, b: 0.28 };  // medium grass green

// Per-category terrain colors. Naturalistes (orientés photo aérienne) plutôt que CO papier.
const TERRAIN_VEG_COLORS = {
  feuillus: { r: 0.16, g: 0.42, b: 0.16 }, // forêt dense feuillus — vert foncé
  conif:    { r: 0.10, g: 0.34, b: 0.16 }, // conifères — encore plus foncé
  mixte:    { r: 0.13, g: 0.38, b: 0.16 },
  bois:     { r: 0.20, g: 0.48, b: 0.20 }, // petits bois — vert moyen
  vigne:    { r: 0.55, g: 0.52, b: 0.28 }, // vignes — tan-vert (signature Beaujolais)
  verger:   { r: 0.38, g: 0.58, b: 0.24 }, // verger
  lande:    { r: 0.52, g: 0.58, b: 0.32 }, // lande
  default:  { r: 0.32, g: 0.55, b: 0.22 }, // végétation inconnue
};

// Deterministic small noise based on integer (i, j) — kills the flat aplat look.
// Returns a value in [-1, 1].
function vertexNoise(i, j) {
  const n = Math.sin(i * 12.9898 + j * 78.233) * 43758.5453;
  return (n - Math.floor(n)) * 2 - 1;
}

// Apply natural variation to a base color: small RGB jitter + slight altitude tint.
// alt=246 → no shift, alt=534 → up to +0.05 brown shift (drier high ground).
function jitterColor(base, i, j, alt, minAlt, maxAlt) {
  const altT = (maxAlt - minAlt > 0) ? (alt - minAlt) / (maxAlt - minAlt) : 0;
  const noise = vertexNoise(i, j) * 0.04;            // ±0.04 RGB jitter
  const altShift = altT * 0.05;                       // +0.05 toward brown at top
  return {
    r: Math.min(1, Math.max(0, base.r + noise + altShift * 0.6)),
    g: Math.min(1, Math.max(0, base.g + noise * 0.8 - altShift * 0.2)),
    b: Math.min(1, Math.max(0, base.b + noise * 0.6 - altShift * 0.4)),
  };
}

function pickTerrainVegColor(nature) {
  if (nature.includes('feuillus')) return TERRAIN_VEG_COLORS.feuillus;
  if (nature.includes('conif')) return TERRAIN_VEG_COLORS.conif;
  if (nature.includes('mixte')) return TERRAIN_VEG_COLORS.mixte;
  if (nature.includes('Bois')) return TERRAIN_VEG_COLORS.bois;
  if (nature.includes('Vigne')) return TERRAIN_VEG_COLORS.vigne;
  if (nature.includes('Verger')) return TERRAIN_VEG_COLORS.verger;
  if (nature.includes('Lande')) return TERRAIN_VEG_COLORS.lande;
  return TERRAIN_VEG_COLORS.default;
}

/**
 * Repaint each terrain vertex according to which vegetation polygon (if any) contains it.
 * Vertices outside any zone keep TERRAIN_BASE_COLOR.
 *
 * Performance: 19600 vertices × ~5-20 zones (after bbox filter) × ~15 PIP edges = ~3M ops.
 * Typical wall-clock under 100ms. The bbox spatial filter avoids the naive 9M ops.
 */
function recolorTerrainFromVegetation(zones) {
  if (!terrainMesh || !terrain) return;
  const t0 = performance.now();

  // Precompute each zone's lat/lon bbox (used as a cheap pre-filter before PIP).
  const zonesWithBbox = zones.map((z) => {
    let minLat = Infinity, maxLat = -Infinity, minLon = Infinity, maxLon = -Infinity;
    for (const c of z.coords) {
      if (c.lat < minLat) minLat = c.lat;
      if (c.lat > maxLat) maxLat = c.lat;
      if (c.lon < minLon) minLon = c.lon;
      if (c.lon > maxLon) maxLon = c.lon;
    }
    return { zone: z, minLat, maxLat, minLon, maxLon, color: pickTerrainVegColor(z.nature) };
  });

  // Iterate every grid vertex and assign a color
  const cellLat = (terrain.cellSizeM) / (EARTH_RADIUS * Math.PI / 180);
  const cellLon = (terrain.cellSizeM) / (EARTH_RADIUS * Math.PI / 180 * Math.cos(terrain.originLat * Math.PI / 180));
  const colorAttr = terrainMesh.geometry.attributes.color;
  let recolored = 0;
  for (let j = 0; j < terrain.rows; j++) {
    // PlaneGeometry j=0 is north (post-rotation), DEM row 0 is south.
    // Vertex (j, i) lat = originLat + (rows-1-j) * cellLat
    const demRow = planeJToDemRow(j, terrain.rows);
    const vertexLat = terrain.originLat + demRow * cellLat;
    for (let i = 0; i < terrain.cols; i++) {
      const vertexLon = terrain.originLon + i * cellLon;
      const vertexIdx = j * terrain.cols + i;

      // Find the FIRST containing zone (priority by iteration order).
      // Forest takes priority over scattered features because zones are typically
      // disjoint anyway. If two zones overlap, the first wins.
      let found = null;
      for (const zb of zonesWithBbox) {
        if (vertexLat < zb.minLat || vertexLat > zb.maxLat) continue;
        if (vertexLon < zb.minLon || vertexLon > zb.maxLon) continue;
        if (pointInPolygon(vertexLat, vertexLon, zb.zone.coords)) {
          found = zb;
          break;
        }
      }
      if (found) {
        const alt = terrain.gridArray[demRow][i];
        const c = jitterColor(found.color, i, j, alt, terrain.minAlt, terrain.maxAlt);
        colorAttr.setXYZ(vertexIdx, c.r, c.g, c.b);
        recolored++;
      }
    }
  }
  colorAttr.needsUpdate = true;
  const dt = performance.now() - t0;
  console.log(`[world3d] terrain recolor: ${recolored}/${terrain.rows * terrain.cols} vertices in ${dt.toFixed(0)}ms`);
}

// ============================================================
// Roads
// ============================================================

// Realistic rural road widths for the Beaujolais area.
// Major road ~4m, chemin ~2.5m, sentier ~1.5m. Buildings sit very close to roads
// in French hamlets so generous widths cause visual overlap.
const ROAD_STYLES = {
  major:   { color: 0x222222, halfWidth: 2.0 }, // dark asphalt — 4m wide
  chemin:  { color: 0xc97a3a, halfWidth: 1.25 }, // orange — 2.5m wide
  sentier: { color: 0xffd54a, halfWidth: 0.8 }, // yellow — 1.6m wide
  cyclable:{ color: 0xff66cc, halfWidth: 1.0 }, // pink — 2m wide
  default: { color: 0xb88a52, halfWidth: 1.2 },
};

function pickRoadStyle(nature) {
  if (nature.includes('1 chauss') || nature.includes('2 chauss') || nature.includes('Rond-point')) return ROAD_STYLES.major;
  if (nature.includes('Chemin')) return ROAD_STYLES.chemin;
  if (nature.includes('Sentier')) return ROAD_STYLES.sentier;
  if (nature.includes('cyclable')) return ROAD_STYLES.cyclable;
  return ROAD_STYLES.default;
}

export function setRoads(roads) {
  console.log('[world3d] setRoads', roads.length);
  lastRoadsPayload = roads;
  if (!scene) {
    console.log('[world3d] setRoads deferred (scene not ready)');
    return;
  }
  applyRoads(roads);
}

function applyRoads(roads) {
  if (roadGroup) {
    disposeGroup(roadGroup);
    scene.remove(roadGroup);
  }
  roadGroup = new THREE.Group();
  roadGroup.name = 'roads';

  // Build the road quad strips
  const playerXZ = latLonToXZ(player.lat, player.lon);
  let nearestRoadDist = Infinity;
  let nearestRoadNature = '';

  for (const road of roads) {
    if (road.coords.length < 2) continue;
    for (const c of road.coords) {
      const xz = latLonToXZ(c.lat, c.lon);
      const d = Math.hypot(xz.x - playerXZ.x, xz.z - playerXZ.z);
      if (d < nearestRoadDist) {
        nearestRoadDist = d;
        nearestRoadNature = road.nature;
      }
    }
    const style = pickRoadStyle(road.nature);
    const mesh = buildRoadMesh(road.coords, style);
    if (mesh) roadGroup.add(mesh);
  }

  // Add rounded junction discs at every endpoint cluster — fills the gap when
  // two roads meet, and rounds out the abrupt square ends of isolated road tips.
  const junctionMeshes = buildJunctionDiscs(roads);
  for (const m of junctionMeshes) roadGroup.add(m);

  console.log('[world3d] DIAG nearest road to player:',
              nearestRoadDist.toFixed(1), 'm,', 'nature:', nearestRoadNature,
              '— player(lat,lon)=', player.lat.toFixed(6), ',', player.lon.toFixed(6),
              '— sceneXZ=', playerXZ.x.toFixed(1), ',', playerXZ.z.toFixed(1));

  scene.add(roadGroup);
}

/**
 * Generate rounded discs at every road endpoint cluster.
 *
 * 1. Collect (x, z) of every road endpoint with its style.
 * 2. Cluster endpoints whose centers are within MERGE_RADIUS of each other
 *    (BFS over a spatial hash grid — O(N) total).
 * 3. For each cluster, place ONE flat disc at the cluster centroid, with
 *    radius = max halfWidth of any road touching the cluster, color = same.
 * 4. All discs of the same color are merged into a single BufferGeometry to
 *    keep draw calls minimal (5 colors → 5 draw calls regardless of road count).
 *
 * Runs in <50ms for ~650 roads / ~1300 endpoints.
 */
function buildJunctionDiscs(roads) {
  const t0 = performance.now();
  const MERGE_RADIUS = 4.0;
  const MERGE_RADIUS_SQ = MERGE_RADIUS * MERGE_RADIUS;
  const SEGMENTS = 14; // disc tessellation — 14 looks smooth without bloating draw

  // 1. Collect endpoints
  const endpoints = [];
  for (const road of roads) {
    if (road.coords.length < 2) continue;
    const style = pickRoadStyle(road.nature);
    const first = road.coords[0];
    const last = road.coords[road.coords.length - 1];
    const f = latLonToXZ(first.lat, first.lon);
    const l = latLonToXZ(last.lat, last.lon);
    endpoints.push({ x: f.x, z: f.z, lat: first.lat, lon: first.lon, halfWidth: style.halfWidth, color: style.color, cluster: -1 });
    endpoints.push({ x: l.x, z: l.z, lat: last.lat, lon: last.lon, halfWidth: style.halfWidth, color: style.color, cluster: -1 });
  }

  // 2. Spatial hash grid for cluster BFS
  const grid = new Map();
  const cellOf = (x, z) => `${Math.floor(x / MERGE_RADIUS)},${Math.floor(z / MERGE_RADIUS)}`;
  for (let i = 0; i < endpoints.length; i++) {
    const k = cellOf(endpoints[i].x, endpoints[i].z);
    if (!grid.has(k)) grid.set(k, []);
    grid.get(k).push(i);
  }

  // BFS clustering
  const clusters = [];
  for (let i = 0; i < endpoints.length; i++) {
    if (endpoints[i].cluster !== -1) continue;
    const id = clusters.length;
    const members = [];
    const queue = [i];
    endpoints[i].cluster = id;
    while (queue.length > 0) {
      const idx = queue.pop();
      members.push(idx);
      const cx = Math.floor(endpoints[idx].x / MERGE_RADIUS);
      const cz = Math.floor(endpoints[idx].z / MERGE_RADIUS);
      for (let dx = -1; dx <= 1; dx++) {
        for (let dz = -1; dz <= 1; dz++) {
          const cell = grid.get(`${cx + dx},${cz + dz}`);
          if (!cell) continue;
          for (const j of cell) {
            if (endpoints[j].cluster !== -1) continue;
            const ddx = endpoints[j].x - endpoints[idx].x;
            const ddz = endpoints[j].z - endpoints[idx].z;
            if (ddx * ddx + ddz * ddz <= MERGE_RADIUS_SQ) {
              endpoints[j].cluster = id;
              queue.push(j);
            }
          }
        }
      }
    }
    clusters.push(members);
  }

  // 3. Per cluster: centroid, max halfWidth, dominant color (largest radius wins)
  // 4. Group by color → merged BufferGeometry per color
  const positionsByColor = new Map();
  const indicesByColor = new Map();
  const yOffset = 0.18; // a hair above the road quads (yOffset=0.15) so caps sit on top

  for (const cluster of clusters) {
    let cx = 0, cz = 0;
    let maxR = 0;
    let dominantColor = 0;
    let cLat = 0, cLon = 0;
    for (const idx of cluster) {
      const ep = endpoints[idx];
      cx += ep.x;
      cz += ep.z;
      cLat += ep.lat;
      cLon += ep.lon;
      if (ep.halfWidth > maxR) {
        maxR = ep.halfWidth;
        dominantColor = ep.color;
      }
    }
    cx /= cluster.length;
    cz /= cluster.length;
    cLat /= cluster.length;
    cLon /= cluster.length;
    const cy = sampleAltTri(cLat, cLon) + yOffset;

    if (!positionsByColor.has(dominantColor)) {
      positionsByColor.set(dominantColor, []);
      indicesByColor.set(dominantColor, []);
    }
    const pos = positionsByColor.get(dominantColor);
    const idx = indicesByColor.get(dominantColor);

    // Triangle fan: center vertex + SEGMENTS rim vertices
    const baseIdx = pos.length / 3;
    pos.push(cx, cy, cz); // center
    for (let s = 0; s < SEGMENTS; s++) {
      const a = (s / SEGMENTS) * Math.PI * 2;
      pos.push(cx + Math.cos(a) * maxR, cy, cz + Math.sin(a) * maxR);
    }
    for (let s = 0; s < SEGMENTS; s++) {
      const a = baseIdx + 1 + s;
      const b = baseIdx + 1 + ((s + 1) % SEGMENTS);
      idx.push(baseIdx, a, b);
    }
  }

  // 5. Build one mesh per color
  const meshes = [];
  for (const [color, pos] of positionsByColor) {
    const idx = indicesByColor.get(color);
    if (pos.length === 0) continue;
    const geom = new THREE.BufferGeometry();
    geom.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    geom.setIndex(idx);
    geom.computeVertexNormals();
    const mat = new THREE.MeshLambertMaterial({
      color,
      side: THREE.DoubleSide,
      polygonOffset: true,
      polygonOffsetFactor: -3,
      polygonOffsetUnits: -3,
    });
    const mesh = new THREE.Mesh(geom, mat);
    mesh.name = 'road-junctions-' + color.toString(16);
    meshes.push(mesh);
  }

  const dt = performance.now() - t0;
  console.log(`[world3d] road junctions: ${clusters.length} discs in ${meshes.length} meshes (${dt.toFixed(0)}ms)`);
  return meshes;
}

/**
 * Subdivise les segments de route trop longs pour que le quad strip suive
 * le relief triangulé du terrain. IGN livre des sommets espacés de 10-30m,
 * mais le terrain a des cellules de 25m → entre deux sommets IGN, la route
 * peut survoler une bosse. Avec MAX_SEG_LEN_M = 4m, on a ~6 sub-points par
 * cellule terrain et le quad strip colle de très près au mesh.
 *
 * Note : on garde TOUS les sommets IGN d'origine et on ajoute uniquement
 * des points intermédiaires. Les endpoints (utilisés par buildJunctionDiscs)
 * restent strictement les premiers/derniers coords.
 */
function subdivideRoadCoords(coords, maxSegLenM) {
  if (coords.length < 2) return coords;
  const result = [coords[0]];
  for (let i = 1; i < coords.length; i++) {
    const a = coords[i - 1];
    const b = coords[i];
    const xzA = latLonToXZ(a.lat, a.lon);
    const xzB = latLonToXZ(b.lat, b.lon);
    const dist = Math.hypot(xzB.x - xzA.x, xzB.z - xzA.z);
    if (dist > maxSegLenM) {
      const n = Math.ceil(dist / maxSegLenM);
      for (let k = 1; k < n; k++) {
        const t = k / n;
        result.push({
          lat: a.lat + (b.lat - a.lat) * t,
          lon: a.lon + (b.lon - a.lon) * t,
        });
      }
    }
    result.push(b);
  }
  return result;
}

/** Build a textured road quad strip following the polyline, draped on terrain. */
function buildRoadMesh(coords, style) {
  // Subdivision pour coller au relief : sub-points tous les 4m max.
  // Chaque sub-point ré-échantillonne l'altitude via sampleAltTri (qui matche
  // le mesh terrain au flottant près) → la route suit la triangulation au lieu
  // de tendre une corde rectiligne au-dessus des bosses.
  const dense = subdivideRoadCoords(coords, 4.0);

  // Triangulated sample — matches EXACTLY the rendered terrain mesh (zero mismatch).
  // Pure math, same cost as bilinear, no raycast needed.
  const pts = dense.map((c) => {
    const { x, z } = latLonToXZ(c.lat, c.lon);
    const y = sampleAltTri(c.lat, c.lon);
    return { x, y, z };
  });

  if (pts.length < 2) return null;

  // Build left/right edges using miter normal at each vertex
  const positions = [];
  const indices = [];

  for (let i = 0; i < pts.length; i++) {
    const p = pts[i];
    const prev = pts[Math.max(0, i - 1)];
    const next = pts[Math.min(pts.length - 1, i + 1)];

    // Average tangent direction in XZ
    let tx = next.x - prev.x;
    let tz = next.z - prev.z;
    const tlen = Math.hypot(tx, tz);
    if (tlen < 1e-3) { tx = 1; tz = 0; }
    else { tx /= tlen; tz /= tlen; }

    // Normal = perpendicular in XZ
    const nx = -tz;
    const nz = tx;

    // Avec sampleAltTri, l'altitude matche le mesh à la précision flottante près.
    // yOffset minimal juste pour éviter le z-fighting avec le terrain.
    const yOffset = 0.15;

    positions.push(p.x + nx * style.halfWidth, p.y + yOffset, p.z + nz * style.halfWidth); // left
    positions.push(p.x - nx * style.halfWidth, p.y + yOffset, p.z - nz * style.halfWidth); // right
  }

  for (let i = 0; i < pts.length - 1; i++) {
    const a = i * 2;
    // Winding chosen so the geometric normal points UP (+Y).
    // Vertices: a=south0, a+1=north0, a+2=south1, a+3=north1.
    // T1=(south0, south1, north0): normal = (south1-south0)×(north0-south0)
    //    = forward × (-2nz_perp) → +Y for east-going road. Same for general direction.
    // T2=(north0, south1, north1): forms the other half of the quad with consistent winding.
    indices.push(a, a + 2, a + 1, a + 1, a + 2, a + 3);
  }

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setIndex(indices);
  geometry.computeVertexNormals();

  // Normal lit material — no more X-ray. Roads are properly occluded by hills.
  // polygonOffset prevents z-fighting with the terrain plane on flat sections.
  // DoubleSide because the quad winding (left/right vertex order) makes the
  // computed normal point DOWN — without DoubleSide, roads are backface-culled
  // when viewed from above, which is exactly the player's perspective.
  // This was a latent bug masked by X-ray mode (which forced render order regardless).
  const material = new THREE.MeshLambertMaterial({
    color: style.color,
    side: THREE.DoubleSide,
    polygonOffset: true,
    polygonOffsetFactor: -2,
    polygonOffsetUnits: -2,
  });

  const mesh = new THREE.Mesh(geometry, material);
  return mesh;
}

// ============================================================
// Vegetation (instanced trees per zone)
// ============================================================

const VEGETATION_COLORS = {
  feuillus: 0x1e641e,
  conif:    0x145028,
  mixte:    0x195a23,
  bois:     0x236923,
  vigne:    0x8c6e46,
  verger:   0x64963c,
  lande:    0x8ca050,
  default:  0x3c7832,
};

function pickVegColor(nature) {
  if (nature.includes('feuillus')) return VEGETATION_COLORS.feuillus;
  if (nature.includes('conif')) return VEGETATION_COLORS.conif;
  if (nature.includes('mixte')) return VEGETATION_COLORS.mixte;
  if (nature.includes('Bois')) return VEGETATION_COLORS.bois;
  if (nature.includes('Vigne')) return VEGETATION_COLORS.vigne;
  if (nature.includes('Verger')) return VEGETATION_COLORS.verger;
  if (nature.includes('Lande')) return VEGETATION_COLORS.lande;
  return VEGETATION_COLORS.default;
}

export function setVegetation(zones) {
  console.log('[world3d] setVegetation', zones.length);
  lastVegetationPayload = zones;
  if (!scene) {
    console.log('[world3d] setVegetation deferred (scene not ready)');
    return;
  }
  applyVegetation(zones);
}

function applyVegetation(zones) {
  if (vegetationGroup) {
    disposeGroup(vegetationGroup);
    scene.remove(vegetationGroup);
  }
  vegetationGroup = new THREE.Group();
  vegetationGroup.name = 'vegetation';

  // Recolor terrain vertices according to vegetation zones (if terrain mesh exists).
  // No-op if terrain hasn't arrived yet — applyTerrain will handle it via lastVegetationPayload.
  recolorTerrainFromVegetation(zones);

  // Two rendering paths:
  //   - "vine" zones (Vigne): low green bushes in tighter spacing, no trunk
  //   - "tree" zones (forest, etc): regular trunk + crown, wider spacing
  // HARD CAPS to prevent browser freeze when there are many large zones.
  const MAX_VINES = 8000;
  const MAX_TREES = 15000;
  const vinePositions = [];
  const treesByColor = new Map();
  let treeCount = 0;

  for (const zone of zones) {
    if (zone.coords.length < 3) continue;
    const isVine = zone.nature.includes('Vigne');
    if (isVine) {
      if (vinePositions.length >= MAX_VINES) continue;
      const samples = sampleTreesInPolygon(zone.coords, 12); // ~12m spacing
      vinePositions.push(...samples.slice(0, MAX_VINES - vinePositions.length));
    } else {
      if (treeCount >= MAX_TREES) continue;
      const color = pickVegColor(zone.nature);
      const samples = sampleTreesInPolygon(zone.coords, 16); // ~16m spacing (sparser)
      const room = MAX_TREES - treeCount;
      const slice = samples.slice(0, room);
      if (!treesByColor.has(color)) treesByColor.set(color, []);
      treesByColor.get(color).push(...slice);
      treeCount += slice.length;
    }
  }
  console.log('[world3d] vegetation:', vinePositions.length, 'vines,', treeCount, 'trees');

  // --- Trees (forest, lande, verger) ---
  const trunkGeo = new THREE.CylinderGeometry(0.15, 0.22, 1.8, 6);
  trunkGeo.translate(0, 0.9, 0);
  const crownGeo = new THREE.SphereGeometry(2.0, 8, 6);
  crownGeo.translate(0, 3.0, 0);
  const trunkMat = new THREE.MeshLambertMaterial({ color: 0x5a3c1e });

  for (const [color, positions] of treesByColor) {
    if (positions.length === 0) continue;
    const crownMat = new THREE.MeshLambertMaterial({ color });
    const trunkMesh = new THREE.InstancedMesh(trunkGeo, trunkMat, positions.length);
    const crownMesh = new THREE.InstancedMesh(crownGeo, crownMat, positions.length);
    const dummy = new THREE.Object3D();
    for (let i = 0; i < positions.length; i++) {
      const p = positions[i];
      dummy.position.set(p.x, p.y, p.z);
      dummy.rotation.y = (i * 1.7) % (Math.PI * 2);
      const scale = 0.85 + ((i * 13) % 100) / 200;
      dummy.scale.set(scale, scale, scale);
      dummy.updateMatrix();
      trunkMesh.setMatrixAt(i, dummy.matrix);
      crownMesh.setMatrixAt(i, dummy.matrix);
    }
    trunkMesh.instanceMatrix.needsUpdate = true;
    crownMesh.instanceMatrix.needsUpdate = true;
    vegetationGroup.add(trunkMesh);
    vegetationGroup.add(crownMesh);
  }

  // --- Vines: small low green bushes (no trunk), no shape rotation ---
  if (vinePositions.length > 0) {
    const vineBushGeo = new THREE.SphereGeometry(0.5, 6, 4);
    vineBushGeo.scale(1.0, 0.6, 1.0); // squashed = bushy
    vineBushGeo.translate(0, 0.5, 0);
    const vineMat = new THREE.MeshLambertMaterial({ color: 0x6a8e3a }); // muted vine green
    const vineMesh = new THREE.InstancedMesh(vineBushGeo, vineMat, vinePositions.length);
    const dummy = new THREE.Object3D();
    for (let i = 0; i < vinePositions.length; i++) {
      const p = vinePositions[i];
      dummy.position.set(p.x, p.y, p.z);
      dummy.scale.set(1, 1, 1);
      dummy.rotation.set(0, 0, 0);
      dummy.updateMatrix();
      vineMesh.setMatrixAt(i, dummy.matrix);
    }
    vineMesh.instanceMatrix.needsUpdate = true;
    vegetationGroup.add(vineMesh);
  }

  scene.add(vegetationGroup);
}

/** Sample tree positions inside a polygon at roughly the given spacing in meters. */
function sampleTreesInPolygon(coords, spacingM) {
  // Bounding box
  let minLat = Infinity, maxLat = -Infinity, minLon = Infinity, maxLon = -Infinity;
  for (const c of coords) {
    if (c.lat < minLat) minLat = c.lat;
    if (c.lat > maxLat) maxLat = c.lat;
    if (c.lon < minLon) minLon = c.lon;
    if (c.lon > maxLon) maxLon = c.lon;
  }

  const latStep = spacingM / 111000;
  const lonStep = spacingM / (111000 * Math.cos(((minLat + maxLat) / 2) * Math.PI / 180));

  const trees = [];
  for (let lat = minLat; lat <= maxLat; lat += latStep) {
    for (let lon = minLon; lon <= maxLon; lon += lonStep) {
      // Jitter to avoid grid look
      const jLat = lat + (Math.sin(lat * 1000 + lon * 999) * latStep * 0.4);
      const jLon = lon + (Math.cos(lat * 999 + lon * 1000) * lonStep * 0.4);
      if (pointInPolygon(jLat, jLon, coords)) {
        const { x, z } = latLonToXZ(jLat, jLon);
        const y = sampleAltTri(jLat, jLon); // matches terrain mesh exactly
        trees.push({ x, y, z });
      }
    }
  }
  return trees;
}

function pointInPolygon(lat, lon, polygon) {
  let inside = false;
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const xi = polygon[i].lon, yi = polygon[i].lat;
    const xj = polygon[j].lon, yj = polygon[j].lat;
    const intersect = ((yi > lat) !== (yj > lat))
      && (lon < (xj - xi) * (lat - yi) / (yj - yi + 1e-12) + xi);
    if (intersect) inside = !inside;
  }
  return inside;
}

// ============================================================
// Buildings
// ============================================================

export function setBuildings(buildings) {
  console.log('[world3d] setBuildings', buildings.length);
  lastBuildingsPayload = buildings;
  if (!scene) {
    console.log('[world3d] setBuildings deferred (scene not ready)');
    return;
  }
  applyBuildings(buildings);
}

function applyBuildings(buildings) {
  if (buildingGroup) {
    disposeGroup(buildingGroup);
    scene.remove(buildingGroup);
  }
  buildingGroup = new THREE.Group();
  buildingGroup.name = 'buildings';

  for (const b of buildings) {
    if (b.coords.length < 3) continue;
    const mesh = buildBuildingMesh(b);
    if (mesh) buildingGroup.add(mesh);
  }

  scene.add(buildingGroup);
}

function buildBuildingMesh(building) {
  const isIndustrial = building.nature.includes('Industriel') || building.nature.includes('agricole');
  const wallColor = isIndustrial ? 0xa09690 : 0xc8b9aa;
  // Reduce IGN-reported building height: factor 0.4, hard cap 8m, floor 2.5m
  // (rural villages — avoid towering structures that mask the landscape).
  const height = Math.min(8, Math.max(2.5, building.hauteur * 0.4));

  // Compute the MIN altitude across all building corners. Anchoring on the
  // FIRST coord causes the building to float on the downhill side or be
  // half-buried on the uphill side. By starting at minAlt and adding extra
  // height to compensate, the building always reaches the ground on every side.
  let minBaseY = Infinity;
  let maxBaseY = -Infinity;
  for (const c of building.coords) {
    const y = sampleAltTri(c.lat, c.lon);
    if (y < minBaseY) minBaseY = y;
    if (y > maxBaseY) maxBaseY = y;
  }
  const slopeM = maxBaseY - minBaseY;
  const totalHeight = height + slopeM; // walls extend to cover the slope

  // Use the FIRST coord's local XZ as the shape origin (Three.js Shape is in 2D).
  const baseLat = building.coords[0].lat;
  const baseLon = building.coords[0].lon;
  const baseXZ = latLonToXZ(baseLat, baseLon);

  // CRITICAL: localY must be NEGATED for the footprint to map correctly to scene Z.
  // After geometry.rotateX(-PI/2), shape vertex (sx, sy, 0) becomes scene (sx, 0, -sy).
  // To place a coord at scene Z = c.z, we need -sy = c.z - baseXZ.z, i.e. sy = baseXZ.z - c.z.
  // Without this negation, the building footprint is mirrored around the first vertex's Z,
  // making the building appear offset by up to its own width from its actual position.
  const shape = new THREE.Shape();
  building.coords.forEach((c, idx) => {
    const { x, z } = latLonToXZ(c.lat, c.lon);
    const localX = x - baseXZ.x;
    const localY = baseXZ.z - z; // negated — see comment above
    if (idx === 0) shape.moveTo(localX, localY);
    else shape.lineTo(localX, localY);
  });

  const extrudeSettings = {
    depth: totalHeight,
    bevelEnabled: false,
    steps: 1,
  };
  const geometry = new THREE.ExtrudeGeometry(shape, extrudeSettings);

  // ExtrudeGeometry extrudes in +Z; rotateX(-PI/2) makes extrusion go +Y (up).
  geometry.rotateX(-Math.PI / 2);

  // DoubleSide as a safety net: the localY negation flips polygon winding,
  // which could invert wall normals depending on IGN's polygon orientation.
  const wallMat = new THREE.MeshLambertMaterial({
    color: wallColor,
    side: THREE.DoubleSide,
  });
  const mesh = new THREE.Mesh(geometry, wallMat);
  // Anchor the base at the LOWEST corner altitude minus 0.3m of safety margin.
  // The walls extend up by `totalHeight` = roof_height + slope, so the roof is
  // flat at minBaseY + totalHeight regardless of slope.
  mesh.position.set(baseXZ.x, minBaseY - 0.3, baseXZ.z);
  return mesh;
}

// ============================================================
// Control Points
// ============================================================

export function setControlPoints(cps) {
  console.log('[world3d] setControlPoints', cps.length);
  lastControlPointsPayload = cps;
  if (!scene) {
    console.log('[world3d] setControlPoints deferred (scene not ready)');
    return;
  }
  applyControlPoints(cps);
}

function applyControlPoints(cps) {
  if (controlPointGroup) {
    disposeGroup(controlPointGroup);
    scene.remove(controlPointGroup);
  }
  controlPointGroup = new THREE.Group();
  controlPointGroup.name = 'controlPoints';

  // White pole 2m + colored sphere "flag" on top.
  // Slightly emissive so they remain visible against dark backgrounds.
  const poleGeo = new THREE.CylinderGeometry(0.06, 0.06, 2.2, 8);
  poleGeo.translate(0, 1.1, 0);
  const poleMat = new THREE.MeshLambertMaterial({ color: 0xffffff });

  const flagGeo = new THREE.SphereGeometry(0.4, 12, 8);
  const flagMatFound = new THREE.MeshBasicMaterial({ color: 0x33cc33 });
  const flagMatPending = new THREE.MeshBasicMaterial({ color: 0xff7700 });

  for (const cp of cps) {
    const { x, z } = latLonToXZ(cp.lat, cp.lon);
    const y = sampleAltTri(cp.lat, cp.lon); // matches terrain mesh exactly

    const pole = new THREE.Mesh(poleGeo, poleMat);
    pole.position.set(x, y, z);
    controlPointGroup.add(pole);

    const flag = new THREE.Mesh(flagGeo, cp.found ? flagMatFound : flagMatPending);
    flag.position.set(x, y + 2.4, z);
    controlPointGroup.add(flag);
  }

  scene.add(controlPointGroup);
}

// ============================================================
// Helpers
// ============================================================

function disposeGroup(group) {
  group.traverse((obj) => {
    if (obj.geometry) obj.geometry.dispose();
    if (obj.material) {
      if (Array.isArray(obj.material)) obj.material.forEach((m) => m.dispose());
      else obj.material.dispose();
    }
  });
}
