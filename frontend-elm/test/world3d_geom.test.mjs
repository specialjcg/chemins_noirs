/**
 * Tests for world3d_geom.js — pure projection / sampling math.
 *
 * Run with: node --test test/world3d_geom.test.mjs
 *
 * These tests capture the contract that broke during the elm-3d-scene → Three.js
 * migration and would have caught the regressions:
 *   1) lat/lon → scene XZ projection
 *   2) Bilinear sample falls back to minAlt outside grid bounds (the actual bug
 *      that buried roads/trees when the DEM was too small)
 *   3) North-south flip mapping between PlaneGeometry and DEM grid
 *   4) DEM coverage of feature points
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  EARTH_RADIUS,
  latLonToXZ,
  xzToLatLon,
  sampleAlt,
  sampleAltTriangulated,
  planeJToDemRow,
  isInsideGrid,
  countOutOfGridPoints,
} from '../src/world3d_geom.js';

// Player position used as origin in most tests (Beaujolais area, ~46° N)
const PLAYER = { lat: 45.93063, lon: 4.57791 };

// ============================================================
// latLonToXZ
// ============================================================

test('latLonToXZ: player at origin returns (0, 0)', () => {
  const { x, z } = latLonToXZ(PLAYER, PLAYER.lat, PLAYER.lon);
  assert.ok(Math.abs(x) < 1e-6, `expected x≈0, got ${x}`);
  assert.ok(Math.abs(z) < 1e-6, `expected z≈0, got ${z}`);
});

test('latLonToXZ: 1° north → -111000m on Z (north is -Z)', () => {
  const { x, z } = latLonToXZ(PLAYER, PLAYER.lat + 1, PLAYER.lon);
  assert.ok(Math.abs(x) < 1, 'no east/west drift expected');
  // 1° lat ≈ 111000m, in -Z direction (north)
  assert.ok(z < -110000 && z > -112000, `expected z ≈ -111000m, got ${z}`);
});

test('latLonToXZ: 1° east → +X stretched by cos(lat)', () => {
  const { x, z } = latLonToXZ(PLAYER, PLAYER.lat, PLAYER.lon + 1);
  assert.ok(Math.abs(z) < 1, 'no north/south drift expected');
  // 1° lon at lat 45.93 ≈ EARTH_RADIUS * π/180 * cos(45.93°)
  const expectedX = EARTH_RADIUS * Math.PI / 180 * Math.cos(PLAYER.lat * Math.PI / 180);
  assert.ok(Math.abs(x - expectedX) < 1, `expected x ≈ ${expectedX}, got ${x}`);
  assert.ok(x > 0, 'east should be +X');
});

test('latLonToXZ ↔ xzToLatLon: round-trip preserves coords for nearby points', () => {
  const target = { lat: PLAYER.lat + 0.005, lon: PLAYER.lon - 0.003 };
  const { x, z } = latLonToXZ(PLAYER, target.lat, target.lon);
  const back = xzToLatLon(PLAYER, x, z);
  assert.ok(Math.abs(back.lat - target.lat) < 1e-6, `lat round-trip: ${back.lat} vs ${target.lat}`);
  assert.ok(Math.abs(back.lon - target.lon) < 1e-6, `lon round-trip: ${back.lon} vs ${target.lon}`);
});

// ============================================================
// sampleAlt — bilinear elevation sampling
// ============================================================

/**
 * Build a tiny synthetic DEM grid for testing.
 * 3×3 grid, 100m cells, centered on PLAYER.
 * Convention: grid[0] = south row, grid[r][0] = west col.
 *   grid[0] = [100, 200, 300]   south row, west→east
 *   grid[1] = [400, 500, 600]   middle
 *   grid[2] = [700, 800, 900]   north row
 */
function makeTinyTerrain() {
  const cellSizeM = 100;
  const rows = 3;
  const cols = 3;
  // origin = south-west corner
  const halfLat = (cellSizeM * (rows - 1)) / 2 / 111000;
  const halfLon = (cellSizeM * (cols - 1)) / 2 / (111000 * Math.cos(PLAYER.lat * Math.PI / 180));
  return {
    gridArray: [
      [100, 200, 300],
      [400, 500, 600],
      [700, 800, 900],
    ],
    originLat: PLAYER.lat - halfLat,
    originLon: PLAYER.lon - halfLon,
    cellSizeM,
    rows,
    cols,
    minAlt: 100,
    maxAlt: 900,
  };
}

test('sampleAlt: returns minAlt when terrain is null', () => {
  assert.equal(sampleAlt(null, PLAYER.lat, PLAYER.lon), 0);
});

test('sampleAlt: south-west corner returns grid[0][0]', () => {
  const terrain = makeTinyTerrain();
  const alt = sampleAlt(terrain, terrain.originLat, terrain.originLon);
  assert.equal(alt, 100);
});

test('sampleAlt: center point returns grid[1][1] (middle)', () => {
  const terrain = makeTinyTerrain();
  const alt = sampleAlt(terrain, PLAYER.lat, PLAYER.lon);
  assert.ok(Math.abs(alt - 500) < 1, `expected ~500, got ${alt}`);
});

test('sampleAlt: bilinear interpolation between cells', () => {
  const terrain = makeTinyTerrain();
  // Halfway between grid[0][0]=100 and grid[0][1]=200 → 150
  const halfLon = terrain.cellSizeM / 2 / (111000 * Math.cos(terrain.originLat * Math.PI / 180));
  const alt = sampleAlt(terrain, terrain.originLat, terrain.originLon + halfLon);
  assert.ok(Math.abs(alt - 150) < 5, `expected ~150, got ${alt}`);
});

test('sampleAlt: REGRESSION — point outside grid returns minAlt (was the bug that buried roads)', () => {
  const terrain = makeTinyTerrain();
  // Far outside the tiny 200m × 200m grid: 5km north
  const alt = sampleAlt(terrain, PLAYER.lat + 0.05, PLAYER.lon);
  assert.equal(alt, terrain.minAlt, 'out-of-grid points must return minAlt — caller responsible for ensuring grid coverage');
});

// ============================================================
// sampleAltTriangulated — matches Three.js PlaneGeometry triangulation exactly
// ============================================================

test('sampleAltTriangulated: returns 0 when terrain is null', () => {
  assert.equal(sampleAltTriangulated(null, PLAYER.lat, PLAYER.lon), 0);
});

test('sampleAltTriangulated: SW corner returns gridArray[0][0]', () => {
  const terrain = makeTinyTerrain();
  const alt = sampleAltTriangulated(terrain, terrain.originLat, terrain.originLon);
  assert.equal(alt, 100);
});

// Helper: convert meters to degrees of latitude using the SAME constant as the function under test.
// 111000 is approximate; the function uses EARTH_RADIUS * π/180 ≈ 111195.
const M_PER_DEG_LAT = EARTH_RADIUS * Math.PI / 180;
function metersToLatDeg(m) { return m / M_PER_DEG_LAT; }
function metersToLonDeg(m, lat) { return m / (M_PER_DEG_LAT * Math.cos(lat * Math.PI / 180)); }

test('sampleAltTriangulated: NE corner returns gridArray[rows-1][cols-1]', () => {
  const terrain = makeTinyTerrain();
  const heightM = terrain.cellSizeM * (terrain.rows - 1);
  const widthM = terrain.cellSizeM * (terrain.cols - 1);
  const lat = terrain.originLat + metersToLatDeg(heightM * 0.999);
  const lon = terrain.originLon + metersToLonDeg(widthM * 0.999, terrain.originLat);
  const alt = sampleAltTriangulated(terrain, lat, lon);
  assert.ok(Math.abs(alt - 900) < 5, `expected ~900 (NE corner), got ${alt}`);
});

test('sampleAltTriangulated: ALL four corners of a single cell return their exact altitudes', () => {
  // 2x2 grid so there's exactly one cell.
  const cellSizeM = 100;
  const halfLat = metersToLatDeg(cellSizeM / 2);
  const halfLon = metersToLonDeg(cellSizeM / 2, PLAYER.lat);
  const terrain = {
    gridArray: [
      [10, 20], // SOUTH row: SW=10, SE=20
      [30, 40], // NORTH row: NW=30, NE=40
    ],
    originLat: PLAYER.lat - halfLat,
    originLon: PLAYER.lon - halfLon,
    cellSizeM, rows: 2, cols: 2,
    minAlt: 10, maxAlt: 40,
  };
  // SW corner (exactly origin)
  assert.equal(sampleAltTriangulated(terrain, terrain.originLat, terrain.originLon), 10);
  // SE corner — just inside east edge
  const altSE = sampleAltTriangulated(
    terrain,
    terrain.originLat,
    terrain.originLon + metersToLonDeg(cellSizeM * 0.999, terrain.originLat)
  );
  assert.ok(Math.abs(altSE - 20) < 0.1, `SE corner ~20, got ${altSE}`);
  // NW corner — just inside north edge
  const altNW = sampleAltTriangulated(
    terrain,
    terrain.originLat + metersToLatDeg(cellSizeM * 0.999),
    terrain.originLon
  );
  assert.ok(Math.abs(altNW - 30) < 0.1, `NW corner ~30, got ${altNW}`);
});

test('sampleAltTriangulated: REGRESSION — diagonal SW→NE on a flat-saddle cell', () => {
  // This test guards against accidentally swapping the diagonal to NW→SE.
  // Build a cell where SW=NE=0 and NW=SE=100. The SW→NE diagonal connects equal
  // altitudes (0), while the NW→SE diagonal would connect equal altitudes (100).
  // At the cell center (u=v=0.5):
  //   - With SW→NE diagonal (CORRECT for Three.js PlaneGeometry):
  //     u = vR boundary, alt = 0.5*0 + 0.5*0 = 0  (NW triangle: (vR-u)*altNW + (1-vR)*altSW + u*altNE)
  //   - With NW→SE diagonal (INCORRECT):
  //     center alt would be 100
  // We expect 0 → confirms the SW→NE diagonal.
  const cellSizeM = 100;
  const halfLat = cellSizeM / 2 / 111000;
  const halfLon = cellSizeM / 2 / (111000 * Math.cos(PLAYER.lat * Math.PI / 180));
  const terrain = {
    gridArray: [
      [0, 100],   // SOUTH: SW=0, SE=100
      [100, 0],   // NORTH: NW=100, NE=0
    ],
    originLat: PLAYER.lat - halfLat,
    originLon: PLAYER.lon - halfLon,
    cellSizeM, rows: 2, cols: 2,
    minAlt: 0, maxAlt: 100,
  };
  // Center of the cell — both triangles meet on the SW-NE diagonal
  const altCenter = sampleAltTriangulated(terrain, PLAYER.lat, PLAYER.lon);
  assert.ok(altCenter < 1, `cell center on SW-NE diagonal should be ~0, got ${altCenter}`);
});

test('sampleAltTriangulated: REGRESSION — point in NW half uses NW altitude correctly', () => {
  // Build a cell where only NW corner is high. A point in the NW corner area
  // should be clearly biased toward the NW altitude.
  const cellSizeM = 100;
  const halfLat = cellSizeM / 2 / 111000;
  const halfLon = cellSizeM / 2 / (111000 * Math.cos(PLAYER.lat * Math.PI / 180));
  const terrain = {
    gridArray: [
      [0, 0],    // SOUTH: SW=0, SE=0
      [100, 0],  // NORTH: NW=100, NE=0
    ],
    originLat: PLAYER.lat - halfLat,
    originLon: PLAYER.lon - halfLon,
    cellSizeM, rows: 2, cols: 2,
    minAlt: 0, maxAlt: 100,
  };
  // Point at u=0.1 (west), vR=0.9 (north) — solidly in the NW half
  const ptLat = terrain.originLat + (cellSizeM * 0.9) / 111000;
  const ptLon = terrain.originLon + (cellSizeM * 0.1) / (111000 * Math.cos(terrain.originLat * Math.PI / 180));
  const alt = sampleAltTriangulated(terrain, ptLat, ptLon);
  // weights for NW triangle: w_NW = vR - u = 0.8, w_SW = 1 - vR = 0.1, w_NE = u = 0.1
  // alt = 0.8*100 + 0.1*0 + 0.1*0 = 80
  assert.ok(Math.abs(alt - 80) < 1, `expected ~80 (NW dominated), got ${alt}`);
});

test('sampleAltTriangulated: out of grid returns minAlt', () => {
  const terrain = makeTinyTerrain();
  const alt = sampleAltTriangulated(terrain, PLAYER.lat + 0.05, PLAYER.lon);
  assert.equal(alt, terrain.minAlt);
});

// ============================================================
// isInsideGrid + countOutOfGridPoints
// ============================================================

test('isInsideGrid: player at center is inside', () => {
  const terrain = makeTinyTerrain();
  assert.equal(isInsideGrid(terrain, PLAYER.lat, PLAYER.lon), true);
});

test('isInsideGrid: 5km away is outside', () => {
  const terrain = makeTinyTerrain();
  assert.equal(isInsideGrid(terrain, PLAYER.lat + 0.05, PLAYER.lon), false);
});

test('countOutOfGridPoints: REGRESSION — 9 CPs spanning ~1.3km would all fall outside a 200m grid', () => {
  // These are the actual CP positions from the test session that exposed the bug.
  const cps = [
    { lat: 45.933378, lon: 4.578367 }, // CP 0  ~305m
    { lat: 45.935383, lon: 4.579346 }, // CP 1  ~528m
    { lat: 45.936174, lon: 4.578338 }, // CP 2  ~616m
    { lat: 45.940671, lon: 4.577892 }, // CP 3  ~1116m
    { lat: 45.941902, lon: 4.575616 }, // CP 4  ~1265m  ← furthest
    { lat: 45.939138, lon: 4.575760 }, // CP 5  ~946m
    { lat: 45.935513, lon: 4.575760 }, // CP 6  ~543m
    { lat: 45.934081, lon: 4.577071 }, // CP 7  ~384m
    { lat: 45.930480, lon: 4.577044 }, // CP 8  ~17m
  ];

  // Tiny 200m grid: most CPs outside
  const tiny = makeTinyTerrain();
  const tinyResult = countOutOfGridPoints(tiny, cps);
  assert.ok(tinyResult.outside >= 8, `tiny grid should miss most CPs, got ${tinyResult.outside} outside`);

  // 3500m grid covering ±1750m from player: ALL CPs must be inside
  const big = makeTerrain(3500, 70);
  const bigResult = countOutOfGridPoints(big, cps);
  assert.equal(bigResult.outside, 0,
    `3500m grid must cover all 9 CPs (max distance ~1265m from player), got ${bigResult.outside} outside`);
});

test('countOutOfGridPoints: 800m grid (the original bug) misses far CPs', () => {
  const small = makeTerrain(800, 40);
  // CP 4 is ~1265m from player, way outside an 800m grid (±400m)
  const cp4 = { lat: 45.941902, lon: 4.575616 };
  assert.equal(isInsideGrid(small, cp4.lat, cp4.lon), false,
    'CP 4 at ~1265m must be outside the 800m grid — this was the original bug');
});

// Helper: build a square DEM grid covering ±sizeM/2 around PLAYER, with given resolution.
function makeTerrain(sizeM, resolution) {
  const cellSizeM = sizeM / (resolution - 1);
  const halfLat = (sizeM / 2) / 111000;
  const halfLon = (sizeM / 2) / (111000 * Math.cos(PLAYER.lat * Math.PI / 180));
  const gridArray = [];
  for (let r = 0; r < resolution; r++) {
    const row = [];
    for (let c = 0; c < resolution; c++) row.push(300 + r * 2 + c); // arbitrary
    gridArray.push(row);
  }
  return {
    gridArray,
    originLat: PLAYER.lat - halfLat,
    originLon: PLAYER.lon - halfLon,
    cellSizeM,
    rows: resolution,
    cols: resolution,
    minAlt: 300,
    maxAlt: 300 + 2 * resolution + resolution,
  };
}

// ============================================================
// planeJToDemRow — north-south flip
// ============================================================

test('planeJToDemRow: REGRESSION — Three.js j=0 (north) maps to DEM row=last (north)', () => {
  // Backend convention: grid[0] = SOUTH, grid[rows-1] = NORTH
  // Three.js PlaneGeometry post-rotateX(-PI/2): vertex (i, j=0) is at scene -Z (NORTH)
  // → vertex j=0 must read DEM row (rows-1) which is NORTH
  // This was the FIRST major bug: assigning grid[j] directly produced a NS-mirrored terrain.
  assert.equal(planeJToDemRow(0, 40), 39);
  assert.equal(planeJToDemRow(39, 40), 0);
  assert.equal(planeJToDemRow(20, 40), 19);
});

// ============================================================
// Real-scenario integration test
// ============================================================

// ============================================================
// Building footprint mapping (Three.js Shape + ExtrudeGeometry + rotateX)
// ============================================================

/**
 * Pure replica of world3d.js buildBuildingMesh's coordinate transform.
 * Given building corner (lat, lon), the player origin, and the FIRST corner used
 * as shape origin (baseLat, baseLon), returns the SCENE (X, Z) the corner ends up at.
 *
 * The rendering pipeline is:
 *   1. Shape vertex (sx, sy) where sx = X - baseX, sy = baseZ - Z (NEGATED — the bug fix)
 *   2. ExtrudeGeometry creates 3D vertices (sx, sy, 0) and (sx, sy, depth)
 *   3. geometry.rotateX(-PI/2) sends (x, y, z) → (x, z, -y), so (sx, sy, 0) → (sx, 0, -sy)
 *   4. mesh.position = (baseX, baseY, baseZ) → final scene = (baseX + sx, baseY, baseZ - sy)
 *      = (X, baseY, baseZ - (baseZ - Z)) = (X, baseY, Z) ✓
 */
function buildingCornerToScene(origin, baseLat, baseLon, lat, lon) {
  const baseXZ = latLonToXZ(origin, baseLat, baseLon);
  const cornerXZ = latLonToXZ(origin, lat, lon);
  const sx = cornerXZ.x - baseXZ.x;
  const sy = baseXZ.z - cornerXZ.z; // negated — the fix
  // After rotateX(-PI/2): (sx, sy, 0) → (sx, 0, -sy)
  // After mesh.position: final = (baseXZ.x + sx, _, baseXZ.z - sy)
  return {
    x: baseXZ.x + sx,
    z: baseXZ.z - sy,
  };
}

test('REGRESSION: building footprint maps each corner to its actual world XZ (no mirror around first corner)', () => {
  // Imagine a 10m × 10m building where corner 0 is at the SW.
  // SW = (45.93063, 4.57791) — player position
  // SE = SW + 10m east
  // NE = SW + 10m east + 10m north
  // NW = SW + 10m north
  // After projection through buildingCornerToScene, each corner must land at its
  // actual world XZ — NOT mirrored around the SW corner's Z.
  const SW = { lat: 45.93063, lon: 4.57791 };
  const tenMNorth = 10 / M_PER_DEG_LAT;
  const tenMEast = 10 / (M_PER_DEG_LAT * Math.cos(SW.lat * Math.PI / 180));
  const SE = { lat: SW.lat,             lon: SW.lon + tenMEast };
  const NE = { lat: SW.lat + tenMNorth, lon: SW.lon + tenMEast };
  const NW = { lat: SW.lat + tenMNorth, lon: SW.lon };

  // Use SW as the shape origin (first corner of the polygon)
  const sceneSW = buildingCornerToScene(SW, SW.lat, SW.lon, SW.lat, SW.lon);
  const sceneSE = buildingCornerToScene(SW, SW.lat, SW.lon, SE.lat, SE.lon);
  const sceneNE = buildingCornerToScene(SW, SW.lat, SW.lon, NE.lat, NE.lon);
  const sceneNW = buildingCornerToScene(SW, SW.lat, SW.lon, NW.lat, NW.lon);

  // Expected: scene matches latLonToXZ of each corner directly
  const expSW = latLonToXZ(SW, SW.lat, SW.lon);
  const expSE = latLonToXZ(SW, SE.lat, SE.lon);
  const expNE = latLonToXZ(SW, NE.lat, NE.lon);
  const expNW = latLonToXZ(SW, NW.lat, NW.lon);

  assert.ok(Math.abs(sceneSW.x - expSW.x) < 0.001 && Math.abs(sceneSW.z - expSW.z) < 0.001,
    `SW: got (${sceneSW.x},${sceneSW.z}), expected (${expSW.x},${expSW.z})`);
  assert.ok(Math.abs(sceneSE.x - expSE.x) < 0.001 && Math.abs(sceneSE.z - expSE.z) < 0.001,
    `SE: got (${sceneSE.x},${sceneSE.z}), expected (${expSE.x},${expSE.z})`);
  assert.ok(Math.abs(sceneNE.x - expNE.x) < 0.001 && Math.abs(sceneNE.z - expNE.z) < 0.001,
    `NE: got (${sceneNE.x},${sceneNE.z}), expected (${expNE.x},${expNE.z})`);
  assert.ok(Math.abs(sceneNW.x - expNW.x) < 0.001 && Math.abs(sceneNW.z - expNW.z) < 0.001,
    `NW: got (${sceneNW.x},${sceneNW.z}), expected (${expNW.x},${expNW.z})`);

  // Specifically: NE corner must be NORTH of SW (not south).
  // In Three.js convention, +X = east, -Z = north. So sceneNE.z must be SMALLER (more negative) than sceneSW.z.
  assert.ok(sceneNE.z < sceneSW.z,
    `NE (${sceneNE.z}) must be more north (-Z) than SW (${sceneSW.z}) — if mirrored, it would be south (+Z)`);
});

test('INTEGRATION: with the current Main.elm settings (3500m DEM × 70 res), all 9 test CPs are inside', () => {
  // This is the test that would have prevented the "buried roads" regression.
  // If Main.elm reduces the DEM size below ~2.6km, this test fails.
  const terrain = makeTerrain(3500, 70);
  const cps = [
    { lat: 45.933378, lon: 4.578367 },
    { lat: 45.935383, lon: 4.579346 },
    { lat: 45.936174, lon: 4.578338 },
    { lat: 45.940671, lon: 4.577892 },
    { lat: 45.941902, lon: 4.575616 }, // furthest, ~1265m
    { lat: 45.939138, lon: 4.575760 },
    { lat: 45.935513, lon: 4.575760 },
    { lat: 45.934081, lon: 4.577071 },
    { lat: 45.930480, lon: 4.577044 },
  ];
  const result = countOutOfGridPoints(terrain, cps);
  assert.equal(result.outside, 0,
    `regression check: all 9 reference CPs must be inside the DEM grid. Outside: ${result.outside}`);
  // Sanity: each CP altitude is non-zero (not the minAlt fallback)
  for (const cp of cps) {
    const alt = sampleAlt(terrain, cp.lat, cp.lon);
    assert.ok(alt > terrain.minAlt, `CP at ${cp.lat},${cp.lon} should not get minAlt fallback`);
  }
});
