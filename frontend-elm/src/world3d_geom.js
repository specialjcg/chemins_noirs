/**
 * world3d_geom.js — pure geometry / projection math for the Three.js renderer.
 *
 * NO Three.js dependency, NO module-level state.
 * Every function is pure: same inputs → same outputs.
 *
 * Tested in test/world3d_geom.test.mjs to prevent regressions on:
 *   - lat/lon → scene XZ projection
 *   - elevation grid bilinear sampling
 *   - DEM grid → PlaneGeometry vertex altitude mapping (north-south flip)
 *   - bbox coverage of game features (CPs / roads / vegetation / buildings) by the DEM
 */

export const EARTH_RADIUS = 6371000;

/**
 * Convert lat/lon to local scene meters relative to a player origin.
 * Convention: +X = east, -Z = north (Three.js Y-up).
 * @param {{lat:number, lon:number}} origin - player position
 * @param {number} lat
 * @param {number} lon
 * @returns {{x:number, z:number}}
 */
export function latLonToXZ(origin, lat, lon) {
  const avgLat = origin.lat * Math.PI / 180;
  const x = (lon - origin.lon) * Math.PI / 180 * EARTH_RADIUS * Math.cos(avgLat);
  const z = -(lat - origin.lat) * Math.PI / 180 * EARTH_RADIUS;
  return { x, z };
}

/**
 * Convert scene XZ back to lat/lon relative to a player origin.
 * Inverse of latLonToXZ. Useful for raycast fallback.
 */
export function xzToLatLon(origin, x, z) {
  const avgLat = origin.lat * Math.PI / 180;
  const lon = origin.lon + x / (Math.PI / 180 * EARTH_RADIUS * Math.cos(avgLat));
  const lat = origin.lat - z / (Math.PI / 180 * EARTH_RADIUS);
  return { lat, lon };
}

/**
 * Bilinear sample of an elevation grid at (lat, lon).
 * Returns terrain.minAlt if the point is outside the grid bounds.
 *
 * Backend convention (see backend_partial.rs:519):
 *   grid[0]      = SOUTH row (lowest lat)
 *   grid[rows-1] = NORTH row (highest lat)
 *   grid[r][0]   = WEST col (lowest lon)
 *   grid[r][cols-1] = EAST col (highest lon)
 *
 * @param {{
 *   gridArray: number[][],
 *   originLat: number, originLon: number,
 *   cellSizeM: number, rows: number, cols: number,
 *   minAlt: number
 * }} terrain
 * @param {number} lat
 * @param {number} lon
 * @returns {number} altitude in meters above sea level
 */
export function sampleAlt(terrain, lat, lon) {
  if (!terrain) return 0;
  const { gridArray, originLat, originLon, cellSizeM, rows, cols, minAlt } = terrain;

  const avgLat = originLat * Math.PI / 180;
  const dx = (lon - originLon) * Math.PI / 180 * EARTH_RADIUS * Math.cos(avgLat);
  const dy = (lat - originLat) * Math.PI / 180 * EARTH_RADIUS;
  const colF = dx / cellSizeM;
  const rowF = dy / cellSizeM;

  if (rowF < 0 || rowF >= rows - 1 || colF < 0 || colF >= cols - 1) {
    return minAlt;
  }

  const r0 = Math.floor(rowF);
  const c0 = Math.floor(colF);
  const fr = rowF - r0;
  const fc = colF - c0;

  const z00 = gridArray[r0][c0];
  const z10 = gridArray[r0][c0 + 1];
  const z01 = gridArray[r0 + 1][c0];
  const z11 = gridArray[r0 + 1][c0 + 1];

  return z00 * (1 - fr) * (1 - fc) + z10 * (1 - fr) * fc + z01 * fr * (1 - fc) + z11 * fr * fc;
}

/**
 * Sample the elevation grid using the EXACT same triangulation as Three.js
 * PlaneGeometry. Eliminates the bilinear/triangulated mismatch that makes
 * roads/buildings float above or get buried below the visible terrain mesh.
 *
 * Three.js PlaneGeometry triangulates each quad with two triangles sharing
 * the diagonal from SW corner to NE corner (indices (a,b,d) and (b,c,d) in
 * the source where b is SW and d is NE post-rotation).
 *
 * @param {object} terrain - same shape as sampleAlt's terrain
 * @param {number} lat
 * @param {number} lon
 * @returns {number} altitude in meters
 */
export function sampleAltTriangulated(terrain, lat, lon) {
  if (!terrain) return 0;
  const { gridArray, originLat, originLon, cellSizeM, rows, cols, minAlt } = terrain;

  const avgLat = originLat * Math.PI / 180;
  const dx = (lon - originLon) * Math.PI / 180 * EARTH_RADIUS * Math.cos(avgLat);
  const dy = (lat - originLat) * Math.PI / 180 * EARTH_RADIUS;
  const colF = dx / cellSizeM;
  const rowF = dy / cellSizeM;

  if (rowF < 0 || rowF >= rows - 1 || colF < 0 || colF >= cols - 1) {
    return minAlt;
  }

  const R = Math.floor(rowF);  // DEM row of the SW corner of the cell
  const c = Math.floor(colF);  // DEM col of the SW corner
  const u = colF - c;          // 0 = west, 1 = east
  const vR = rowF - R;         // 0 = south, 1 = north (DEM row direction)

  const altSW = gridArray[R][c];
  const altSE = gridArray[R][c + 1];
  const altNW = gridArray[R + 1][c];
  const altNE = gridArray[R + 1][c + 1];

  // Diagonal SW→NE splits the cell. In (u, vR) coordinates the diagonal is u = vR.
  if (u <= vR) {
    // NW half: triangle (NW, SW, NE), barycentric weights derived in
    // test/world3d_geom.test.mjs
    return (vR - u) * altNW + (1 - vR) * altSW + u * altNE;
  } else {
    // SE half: triangle (SW, SE, NE)
    return (1 - u) * altSW + (u - vR) * altSE + vR * altNE;
  }
}

/**
 * Map a (j, i) index in Three.js PlaneGeometry post-rotation to the
 * corresponding row in the backend DEM grid. The PlaneGeometry vertex (i, j=0)
 * lands at scene NORTH after rotateX(-PI/2), but the DEM row 0 is SOUTH.
 *
 * @param {number} j - PlaneGeometry row index (0 = north after rotation)
 * @param {number} rows - total number of rows
 * @returns {number} DEM row index (0 = south)
 */
export function planeJToDemRow(j, rows) {
  return rows - 1 - j;
}

/**
 * Compute whether a given lat/lon point is INSIDE the DEM grid bounds (with margin in meters).
 * Used to detect "out of grid" placements that would fall back to minAlt.
 */
export function isInsideGrid(terrain, lat, lon, marginM = 0) {
  if (!terrain) return false;
  const { originLat, originLon, cellSizeM, rows, cols } = terrain;
  const avgLat = originLat * Math.PI / 180;
  const dx = (lon - originLon) * Math.PI / 180 * EARTH_RADIUS * Math.cos(avgLat);
  const dy = (lat - originLat) * Math.PI / 180 * EARTH_RADIUS;
  const widthM = cellSizeM * (cols - 1);
  const heightM = cellSizeM * (rows - 1);
  return dx >= -marginM && dx <= widthM + marginM && dy >= -marginM && dy <= heightM + marginM;
}

/**
 * Sanity check: given a list of feature points (CPs/roads/etc) and a terrain payload,
 * count how many points fall outside the DEM grid. Used in tests to assert that the
 * DEM is large enough for the game area.
 *
 * @returns {{ inside: number, outside: number, totalCount: number }}
 */
export function countOutOfGridPoints(terrain, points) {
  let inside = 0, outside = 0;
  for (const p of points) {
    if (isInsideGrid(terrain, p.lat, p.lon)) inside++;
    else outside++;
  }
  return { inside, outside, totalCount: points.length };
}
