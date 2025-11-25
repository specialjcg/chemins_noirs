// Custom 3D engine using Three.js (100% free and open-source)
// No tokens, no paid services

import * as THREE from 'three';
import { OrbitControls } from 'https://cdn.jsdelivr.net/npm/three@0.160.0/examples/jsm/controls/OrbitControls.js';

let scene, camera, renderer, controls;
let routeLine;
let currentRouteCoords = [];
let currentElevations = [];
let isAnimating = false;
let animationFrameId = null;
let currentAnimationIndex = 0;

// Constants for conversion
const METERS_PER_DEGREE_LAT = 111000; // Approximate meters per degree of latitude
const SCALE_FACTOR = 10; // Much smaller scale for better visualization
const ELEVATION_SCALE = 3; // Softer exaggeration for natural relief

let terrainMesh = null;
const elevationPanelId = 'elevationPanel';

// Cache for elevation data to avoid redundant API calls (persisted)
const CACHE_KEY_PREFIX = 'elevation_cache_';

function buildCacheKey(latMin, latMax, lonMin, lonMax, vertexCount) {
  return `${CACHE_KEY_PREFIX}${latMin.toFixed(4)}_${latMax.toFixed(4)}_${lonMin.toFixed(4)}_${lonMax.toFixed(4)}_${vertexCount}`;
}

function loadCache(cacheKey, expectedLength) {
  try {
    const raw = localStorage.getItem(cacheKey);
    if (!raw) {
      return null;
    }
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed) && parsed.length === expectedLength) {
      console.log(`[three3d] ⚡ Using cached elevation data (${parsed.length} points)`);
      return parsed;
    }
    return null;
  } catch (err) {
    console.warn('[three3d] Failed to read elevation cache:', err);
    return null;
  }
}

function saveCache(cacheKey, elevations) {
  try {
    localStorage.setItem(cacheKey, JSON.stringify(elevations));
    console.log(`[three3d] Cached ${elevations.length} elevation values`);
  } catch (err) {
    console.warn('[three3d] Failed to persist elevation cache:', err);
  }
}

let isUpdatingRoute = false; // Prevent concurrent route updates

/**
 * Initialize the Three.js 3D scene
 */
export function initThree3D() {
  const container = document.getElementById('three3dContainer');
  if (!container) {
    console.error("[three3d] Container not found");
    return;
  }

  console.log("[three3d] Initializing Three.js 3D engine");

  // Create scene
  scene = new THREE.Scene();
  scene.background = new THREE.Color(0x87CEEB); // Sky blue

  // Create camera
  camera = new THREE.PerspectiveCamera(
    75, // FOV
    container.clientWidth / container.clientHeight,
    0.1,
    10000
  );
  camera.position.set(0, 100, 200);

  // Create renderer
  renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setSize(container.clientWidth, container.clientHeight);
  container.appendChild(renderer.domElement);

  // Enhanced lighting setup for maximum relief visibility
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.4);
  scene.add(ambientLight);

  // Main directional light (sun) from low angle to emphasize terrain
  const directionalLight = new THREE.DirectionalLight(0xffffff, 1.5);
  directionalLight.position.set(200, 80, 100); // Low angle creates strong shadows
  scene.add(directionalLight);

  // Secondary light from opposite side for better relief definition
  const fillLight = new THREE.DirectionalLight(0xffffff, 0.6);
  fillLight.position.set(-150, 60, -100);
  scene.add(fillLight);

  // Top light to prevent complete darkness in valleys
  const topLight = new THREE.DirectionalLight(0xffffff, 0.3);
  topLight.position.set(0, 200, 0);
  scene.add(topLight);

  // Add a ground plane for reference
  const groundGeometry = new THREE.PlaneGeometry(1000, 1000);
  const groundMaterial = new THREE.MeshLambertMaterial({
    color: 0x228B22, // Forest green
    side: THREE.DoubleSide
  });
  const ground = new THREE.Mesh(groundGeometry, groundMaterial);
  ground.rotation.x = -Math.PI / 2;
  ground.position.y = -1; // Slightly below zero
  scene.add(ground);

  // Add grid helper for better spatial understanding
  const gridHelper = new THREE.GridHelper(1000, 50, 0x444444, 0x222222);
  gridHelper.position.y = -0.9;
  scene.add(gridHelper);

  // Add orbit controls for mouse interaction
  controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true; // Smooth camera movements
  controls.dampingFactor = 0.05;
  controls.screenSpacePanning = false; // Pan in horizontal plane
  controls.minDistance = 10;
  controls.maxDistance = 500;
  controls.maxPolarAngle = Math.PI / 2; // Don't allow camera to go below ground

  // Handle window resize
  window.addEventListener('resize', () => {
    if (container.style.display !== 'none') {
      camera.aspect = container.clientWidth / container.clientHeight;
      camera.updateProjectionMatrix();
      renderer.setSize(container.clientWidth, container.clientHeight);
    }
  });

  // Start render loop
  animate();

  console.log("[three3d] Three.js initialized successfully");
}

/**
 * Animation loop for continuous rendering (needed for OrbitControls)
 */
function animate() {
  requestAnimationFrame(animate);

  // Update controls for damping
  if (controls) {
    controls.update();
  }

  // Render the scene
  if (renderer && scene && camera) {
    renderer.render(scene, camera);
  }
}

/**
 * Convert lat/lon/elevation to 3D coordinates
 */
function latLonToXYZ(lat, lon, elevation, centerLat, centerLon) {
  // Calculate relative position from center point
  const x = (lon - centerLon) * METERS_PER_DEGREE_LAT * Math.cos(centerLat * Math.PI / 180) / SCALE_FACTOR;
  const z = -(lat - centerLat) * METERS_PER_DEGREE_LAT / SCALE_FACTOR; // Negative to match Three.js coordinate system
  const y = elevation * ELEVATION_SCALE / SCALE_FACTOR;

  return { x, y, z };
}

/**
 * Fetch real terrain elevation data from Open-Meteo API in batches
 * Uses caching to avoid redundant API calls
 */
async function fetchTerrainElevations(latitudes, longitudes, cacheKey) {
  try {
    // Check cache first (localStorage)
    const cached = loadCache(cacheKey, latitudes.length);
    if (cached) {
      return cached;
    }

    const batchSize = 50; // keep batches small to avoid 429
    const allElevations = [];
    const baseDelay = 1200; // further increase to play nicely with Open-Meteo
    const retryDelay = 2000;
    const maxRetries = 3;

    console.log(`[three3d] Fetching elevation data for ${latitudes.length} points in batches of ${batchSize}...`);

    for (let i = 0; i < latitudes.length; i += batchSize) {
      const batchLats = latitudes.slice(i, i + batchSize);
      const batchLons = longitudes.slice(i, i + batchSize);

      const latStr = batchLats.join(',');
      const lonStr = batchLons.join(',');

      const url = `https://api.open-meteo.com/v1/elevation?latitude=${latStr}&longitude=${lonStr}`;

      console.log(`[three3d] Batch ${Math.floor(i / batchSize) + 1}/${Math.ceil(latitudes.length / batchSize)}`);

      let attempt = 0;
      let success = false;

      // Retry on transient failures (429/5xx)
      while (!success && attempt <= maxRetries) {
        const response = await fetch(url);

        if (response.ok) {
          const data = await response.json();
          if (data.elevation && Array.isArray(data.elevation)) {
            allElevations.push(...data.elevation);
          }
          success = true;
        } else if ((response.status === 429 || response.status >= 500) && attempt < maxRetries) {
          // Rate limited or server error, backoff and retry
          attempt += 1;
          const waitMs = retryDelay * attempt;
          console.warn(`[three3d] Elevation batch retry ${attempt}/${maxRetries} after ${response.status}, waiting ${waitMs}ms`);
          await new Promise(resolve => setTimeout(resolve, waitMs));
        } else {
          throw new Error(`HTTP error! status: ${response.status}`);
        }
      }

      // Delay between batches to avoid rate limiting
      if (i + batchSize < latitudes.length) {
        const jitter = Math.random() * 200;
        await new Promise(resolve => setTimeout(resolve, baseDelay + jitter));
      }
    }

    console.log(`[three3d] Successfully fetched ${allElevations.length} elevation points`);

    // Cache the results
    if (allElevations.length === latitudes.length) {
      saveCache(cacheKey, allElevations);
    }

    return allElevations;
  } catch (error) {
    console.error('[three3d] Failed to fetch elevation data:', error);
    return [];
  }
}

/**
 * Create 3D terrain mesh from route coordinates and elevations
 */
const CLIENT_FALLBACK_SEGMENTS = 20; // when no backend terrain, use modest resolution to avoid 429s

async function createTerrain(
  coords,
  elevations,
  centerLat,
  centerLon,
  segmentsOverride,
  useRealElevation = true
) {
  if (!coords || coords.length === 0) return null;

  // Calculate bounds
  const lats = coords.map(c => c.lat);
  const lons = coords.map(c => c.lon);
  const minLat = Math.min(...lats);
  const maxLat = Math.max(...lats);
  const minLon = Math.min(...lons);
  const maxLon = Math.max(...lons);

  // Add padding around route
  const latPadding = (maxLat - minLat) * 0.3;
  const lonPadding = (maxLon - minLon) * 0.3;

  const segments = segmentsOverride ?? 64;
  const geometry = new THREE.PlaneGeometry(1, 1, segments, segments);

  console.log(`[three3d] Creating terrain with ${(segments + 1) * (segments + 1)} vertices`);

  // Get terrain bounds in 3D space
  const minPos = latLonToXYZ(minLat - latPadding, minLon - lonPadding, 0, centerLat, centerLon);
  const maxPos = latLonToXYZ(maxLat + latPadding, maxLon + lonPadding, 0, centerLat, centerLon);

  const width = maxPos.x - minPos.x;
  const depth = minPos.z - maxPos.z; // Note: z is inverted

  // Scale geometry to match terrain size
  geometry.scale(width, depth, 1);
  geometry.translate((minPos.x + maxPos.x) / 2, 0, (minPos.z + maxPos.z) / 2);

  // Collect all lat/lon pairs for elevation API
  const positions = geometry.attributes.position;
  const terrainLats = [];
  const terrainLons = [];

  for (let i = 0; i < positions.count; i++) {
    const x = positions.getX(i);
    const z = positions.getZ(i);

    // Convert back to lat/lon
    const lon = centerLon + (x * SCALE_FACTOR) / (METERS_PER_DEGREE_LAT * Math.cos(centerLat * Math.PI / 180));
    const lat = centerLat - (z * SCALE_FACTOR) / METERS_PER_DEGREE_LAT;

    terrainLats.push(lat.toFixed(6));
    terrainLons.push(lon.toFixed(6));
  }

  // Build cache key on padded bounds and vertex count
  const cacheKey = buildCacheKey(
    minLat - latPadding,
    maxLat + latPadding,
    minLon - lonPadding,
    maxLon + lonPadding,
    (segments + 1) * (segments + 1)
  );

  // Fetch real elevation data from API (or cache) unless disabled
  const terrainElevations = useRealElevation
    ? await fetchTerrainElevations(terrainLats, terrainLons, cacheKey)
    : [];

  // Apply elevations to vertices
  if (terrainElevations.length === positions.count) {
    console.log(`[three3d] Applying ${terrainElevations.length} real terrain elevations...`);

    let minElev = Infinity;
    let maxElev = -Infinity;

    for (let i = 0; i < positions.count; i++) {
      const elevation = terrainElevations[i] || 0;
      minElev = Math.min(minElev, elevation);
      maxElev = Math.max(maxElev, elevation);
      positions.setY(i, elevation * ELEVATION_SCALE / SCALE_FACTOR);
    }

    console.log(`[three3d] Elevation range: ${minElev.toFixed(1)}m to ${maxElev.toFixed(1)}m (scale: ${ELEVATION_SCALE}x)`);
  } else {
    console.warn(`[three3d] Elevation data mismatch (got ${terrainElevations.length}, expected ${positions.count}), falling back to route-based interpolation`);
    // Fallback to old method if API fails
    for (let i = 0; i < positions.count; i++) {
      const x = positions.getX(i);
      const z = positions.getZ(i);
      const lon = centerLon + (x * SCALE_FACTOR) / (METERS_PER_DEGREE_LAT * Math.cos(centerLat * Math.PI / 180));
      const lat = centerLat - (z * SCALE_FACTOR) / METERS_PER_DEGREE_LAT;

      let nearestElevation = 0;
      let minDistance = Infinity;

      coords.forEach((coord, idx) => {
        const dist = Math.sqrt(
          Math.pow(coord.lat - lat, 2) +
          Math.pow(coord.lon - lon, 2)
        );
        if (dist < minDistance) {
          minDistance = dist;
          nearestElevation = elevations[idx] || 0;
        }
      });

      positions.setY(i, nearestElevation * ELEVATION_SCALE / SCALE_FACTOR);
    }
  }

  // Rotate to horizontal BEFORE computing normals
  geometry.rotateX(-Math.PI / 2);

  // Compute normals AFTER all transformations for correct lighting
  geometry.computeVertexNormals();

  // Mark position attribute as needing update
  positions.needsUpdate = true;

  // Create material with white color to show true satellite texture colors
  const material = new THREE.MeshStandardMaterial({
    color: 0xffffff, // White - won't tint the satellite texture
    wireframe: false,
    roughness: 0.9, // High roughness to reduce shininess and enhance shadows
    metalness: 0.0,
    flatShading: false, // Smooth shading with computed normals for realistic terrain
    side: THREE.DoubleSide
  });

  // Create mesh first
  const mesh = new THREE.Mesh(geometry, material);

  // Load satellite imagery covering the entire terrain area and adjust UVs
  loadTerrainTexture(material, minLat - latPadding, maxLat + latPadding, minLon - lonPadding, maxLon + lonPadding)
    .then(uvBounds => {
      if (uvBounds) {
        // Adjust UV coordinates to match actual tile coverage
        adjustUVMapping(geometry, minLat - latPadding, maxLat + latPadding, minLon - lonPadding, maxLon + lonPadding, uvBounds);
      }
    });

  return mesh;
}

/**
 * Build terrain mesh directly from backend-provided payload (positions/uvs/indices)
 */
async function createTerrainFromPayload(terrain) {
  try {
    const positions = new Float32Array(terrain.positions || []);
    const uvs = new Float32Array(terrain.uvs || []);
    const indices = new Uint32Array(terrain.indices || []);

    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geometry.setAttribute('uv', new THREE.BufferAttribute(uvs, 2));
    geometry.setIndex(new THREE.BufferAttribute(indices, 1));
    geometry.computeVertexNormals();

    const material = new THREE.MeshStandardMaterial({
      color: 0xffffff,
      wireframe: false,
      roughness: 0.9,
      metalness: 0.0,
      flatShading: false,
      side: THREE.DoubleSide
    });

    const mesh = new THREE.Mesh(geometry, material);

    // Load texture based on provided bounds
    const b = terrain.bounds;
    if (b) {
      await loadTerrainTexture(material, b.min_lat, b.max_lat, b.min_lon, b.max_lon)
        .then(uvBounds => {
          if (uvBounds) {
            adjustUVMapping(geometry, b.min_lat, b.max_lat, b.min_lon, b.max_lon, uvBounds);
          }
        });
    }

    console.log(
      `[three3d] Terrain payload: vertices=${positions.length / 3}, elev range=${terrain.min_elevation}..${terrain.max_elevation}`
    );

    return mesh;
  } catch (err) {
    console.warn('[three3d] Failed to build terrain from payload:', err);
    return null;
  }
}

/**
 * Adjust UV mapping to align texture with terrain geometry
 */
function adjustUVMapping(geometry, terrainMinLat, terrainMaxLat, terrainMinLon, terrainMaxLon, uvBounds) {
  const { tileCoverageMinLat, tileCoverageMaxLat, tileCoverageMinLon, tileCoverageMaxLon } = uvBounds;

  // Calculate how the terrain fits within the tile coverage
  const uMin = (terrainMinLon - tileCoverageMinLon) / (tileCoverageMaxLon - tileCoverageMinLon);
  const uMax = (terrainMaxLon - tileCoverageMinLon) / (tileCoverageMaxLon - tileCoverageMinLon);
  const vMin = (terrainMinLat - tileCoverageMinLat) / (tileCoverageMaxLat - tileCoverageMinLat);
  const vMax = (terrainMaxLat - tileCoverageMinLat) / (tileCoverageMaxLat - tileCoverageMinLat);

  console.log(`[three3d] UV adjustment: u[${uMin.toFixed(4)}, ${uMax.toFixed(4)}], v[${vMin.toFixed(4)}, ${vMax.toFixed(4)}]`);

  // Get UV attribute
  const uvAttribute = geometry.attributes.uv;

  // Adjust each UV coordinate
  for (let i = 0; i < uvAttribute.count; i++) {
    let u = uvAttribute.getX(i);
    let v = uvAttribute.getY(i);

    // Remap from [0,1] to actual coverage within tiles
    u = uMin + u * (uMax - uMin);
    v = vMin + v * (vMax - vMin);

    uvAttribute.setXY(i, u, v);
  }

  uvAttribute.needsUpdate = true;

  console.log('[three3d] UV mapping adjusted for perfect texture alignment');
}

/**
 * Load satellite texture tiles for the terrain
 * Returns UV bounds for proper texture alignment
 */
async function loadTerrainTexture(material, minLat, maxLat, minLon, maxLon) {
  try {
    // Choose the highest zoom that keeps tile count reasonable (<=16 tiles)
    const zoomCandidates = [16, 15, 14, 13];
    let zoom = 13;
    let minTileX = 0;
    let maxTileX = 0;
    let minTileY = 0;
    let maxTileY = 0;
    let tilesX = 0;
    let tilesY = 0;

    for (const z of zoomCandidates) {
      const minTx = Math.floor((minLon + 180) / 360 * Math.pow(2, z));
      const maxTx = Math.floor((maxLon + 180) / 360 * Math.pow(2, z));
      const minTy = Math.floor(
        (1 - Math.log(Math.tan(maxLat * Math.PI / 180) + 1 / Math.cos(maxLat * Math.PI / 180)) / Math.PI) /
          2 *
          Math.pow(2, z)
      );
      const maxTy = Math.floor(
        (1 - Math.log(Math.tan(minLat * Math.PI / 180) + 1 / Math.cos(minLat * Math.PI / 180)) / Math.PI) /
          2 *
          Math.pow(2, z)
      );
      const tx = maxTx - minTx + 1;
      const ty = maxTy - minTy + 1;
      if (tx * ty <= 16) {
        zoom = z;
        minTileX = minTx;
        maxTileX = maxTx;
        minTileY = minTy;
        maxTileY = maxTy;
        tilesX = tx;
        tilesY = ty;
        break;
      }
    }

    console.log(`[three3d] Loading ${tilesX}x${tilesY} satellite tiles at zoom ${zoom}`);

    // Calculate exact bounds of tile coverage in lat/lon
    // This is crucial for UV mapping alignment
    const tileToLon = (x, z) => x / Math.pow(2, z) * 360 - 180;
    const tileToLat = (y, z) => {
      const n = Math.PI - 2 * Math.PI * y / Math.pow(2, z);
      return 180 / Math.PI * Math.atan(0.5 * (Math.exp(n) - Math.exp(-n)));
    };

    // Actual tile coverage (tiles cover slightly more than requested area)
    const tileCoverageMinLon = tileToLon(minTileX, zoom);
    const tileCoverageMaxLon = tileToLon(maxTileX + 1, zoom);
    const tileCoverageMinLat = tileToLat(maxTileY + 1, zoom);
    const tileCoverageMaxLat = tileToLat(minTileY, zoom);

    console.log(`[three3d] Tile coverage: lat [${tileCoverageMinLat.toFixed(5)}, ${tileCoverageMaxLat.toFixed(5)}], lon [${tileCoverageMinLon.toFixed(5)}, ${tileCoverageMaxLon.toFixed(5)}]`);

    // Create canvas to combine tiles
    const tileSize = 256;
    const canvas = document.createElement('canvas');
    canvas.width = tilesX * tileSize;
    canvas.height = tilesY * tileSize;
    const ctx = canvas.getContext('2d');

    // Load all tiles
    const tilePromises = [];
    for (let ty = minTileY; ty <= maxTileY; ty++) {
      for (let tx = minTileX; tx <= maxTileX; tx++) {
        const url = `https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/${zoom}/${ty}/${tx}`;
        const x = (tx - minTileX) * tileSize;
        const y = (ty - minTileY) * tileSize;

        tilePromises.push(
          new Promise((resolve, reject) => {
            const img = new Image();
            img.crossOrigin = 'anonymous';
            img.onload = () => {
              ctx.drawImage(img, x, y, tileSize, tileSize);
              resolve();
            };
            img.onerror = () => resolve(); // Continue even if some tiles fail
            img.src = url;
          })
        );
      }
    }

    await Promise.all(tilePromises);

    // Create texture from canvas
    const texture = new THREE.CanvasTexture(canvas);
    texture.wrapS = THREE.ClampToEdgeWrapping;
    texture.wrapT = THREE.ClampToEdgeWrapping;
    texture.minFilter = THREE.LinearFilter;
    if (renderer && renderer.capabilities) {
      texture.anisotropy = renderer.capabilities.getMaxAnisotropy();
    }

    material.map = texture;
    material.needsUpdate = true;

    console.log(`[three3d] Satellite texture loaded (${tilesX}x${tilesY} tiles)`);

    // Return UV bounds for alignment - the texture covers a slightly different area than requested
    return {
      tileCoverageMinLat,
      tileCoverageMaxLat,
      tileCoverageMinLon,
      tileCoverageMaxLon
    };
  } catch (error) {
    console.warn('[three3d] Failed to load satellite texture:', error);
    material.color.setHex(0x8B7355); // Brown terrain color as fallback
    return null;
  }
}

/**
 * Update the 3D route visualization
 */
export async function updateRoute3D(coords, elevations, terrain) {
  if (!scene) {
    console.warn("[three3d] Scene not initialized");
    return;
  }

  // Prevent concurrent updates
  if (isUpdatingRoute) {
    console.log("[three3d] Update already in progress, skipping duplicate call");
    return;
  }

  isUpdatingRoute = true;

  try {
    console.log("[three3d] Updating route with", coords?.length, "points");

    // Store for animation
    currentRouteCoords = coords || [];
    currentElevations = elevations || [];
    renderElevationPanel(currentRouteCoords, currentElevations);

    if (currentRouteCoords.length === 0) {
      return;
    }

    // Remove existing route and terrain
    if (routeLine) {
      scene.remove(routeLine);
      routeLine = null;
    }
    if (terrainMesh) {
      scene.remove(terrainMesh);
      terrainMesh = null;
    }

    // Calculate center point for coordinate conversion (overridden if terrain payload provided)
    let centerLat = currentRouteCoords[Math.floor(currentRouteCoords.length / 2)].lat;
    let centerLon = currentRouteCoords[Math.floor(currentRouteCoords.length / 2)].lon;

    // If backend provided a ready-made terrain mesh, use it directly
    if (terrain && terrain.positions && terrain.indices) {
      centerLat = terrain.center_lat ?? centerLat;
      centerLon = terrain.center_lon ?? centerLon;
      console.log("[three3d] Using backend-provided terrain mesh");
      terrainMesh = await createTerrainFromPayload(terrain);
      if (terrainMesh) {
        scene.add(terrainMesh);
        console.log("[three3d] Terrain (backend) added to scene");
      }
    } else {
    // Create 3D terrain with satellite texture and real elevation data
    console.log("[three3d] Creating terrain mesh with real elevation data...");
    // Use a modest resolution when we have to fetch elevations client-side to avoid 429s
    terrainMesh = await createTerrain(
      currentRouteCoords,
      currentElevations,
      centerLat,
      centerLon,
      CLIENT_FALLBACK_SEGMENTS,
      false // do not hit Open-Meteo when no backend terrain; rely on route interpolation
    );
      if (terrainMesh) {
        scene.add(terrainMesh);
        console.log("[three3d] Terrain added to scene");
      }
    }

    // Create route line with tube geometry for better visibility
    const points = currentRouteCoords.map((coord, idx) => {
      const elevation = currentElevations[idx] || 0;
      const pos = latLonToXYZ(coord.lat, coord.lon, elevation, centerLat, centerLon);
      return new THREE.Vector3(pos.x, pos.y, pos.z);
    });

    // Create a tube geometry for a thick, visible line
    const curve = new THREE.CatmullRomCurve3(points);
    const tubeGeometry = new THREE.TubeGeometry(curve, points.length * 2, 0.5, 8, false);
    const tubeMaterial = new THREE.MeshLambertMaterial({
      color: 0xff6b35, // Bright orange
      emissive: 0xff3300,
      emissiveIntensity: 0.3
    });
    routeLine = new THREE.Mesh(tubeGeometry, tubeMaterial);
    scene.add(routeLine);

    // Add markers at start and end
    const sphereGeometry = new THREE.SphereGeometry(1, 16, 16);

    const startMaterial = new THREE.MeshLambertMaterial({
      color: 0x00ff00,
      emissive: 0x00ff00,
      emissiveIntensity: 0.5
    });
    const startMarker = new THREE.Mesh(sphereGeometry, startMaterial);
    startMarker.position.copy(points[0]);
    scene.add(startMarker);

    const endMaterial = new THREE.MeshLambertMaterial({
      color: 0xff0000,
      emissive: 0xff0000,
      emissiveIntensity: 0.5
    });
    const endMarker = new THREE.Mesh(sphereGeometry, endMaterial);
    endMarker.position.copy(points[points.length - 1]);
    scene.add(endMarker);

    // Position camera to see the whole route from above at angle
    if (points.length > 0) {
      // Calculate bounding box
      const bounds = {
        minX: Math.min(...points.map(p => p.x)),
        maxX: Math.max(...points.map(p => p.x)),
        minY: Math.min(...points.map(p => p.y)),
        maxY: Math.max(...points.map(p => p.y)),
        minZ: Math.min(...points.map(p => p.z)),
        maxZ: Math.max(...points.map(p => p.z))
      };

      const centerX = (bounds.minX + bounds.maxX) / 2;
      const centerY = (bounds.minY + bounds.maxY) / 2;
      const centerZ = (bounds.minZ + bounds.maxZ) / 2;

      const rangeX = bounds.maxX - bounds.minX;
      const rangeZ = bounds.maxZ - bounds.minZ;
      const maxRange = Math.max(rangeX, rangeZ);

      // Position camera at 45-degree angle above the route
      camera.position.set(
        centerX + maxRange * 0.5,
        centerY + maxRange * 1.2,
        centerZ + maxRange * 0.8
      );
      camera.lookAt(centerX, centerY, centerZ);

      // Update controls target to center of route
      if (controls) {
        controls.target.set(centerX, centerY, centerZ);
        controls.update();
      }
    }

    console.log("[three3d] Route rendered");
  } finally {
    // Always release the lock, even if an error occurred
    isUpdatingRoute = false;
  }
}

/**
 * Render a small overlay with altitudes for each route point
 */
function renderElevationPanel(coords, elevations) {
  const panel = document.getElementById(elevationPanelId);
  if (!panel) {
    return;
  }

  if (!coords || coords.length === 0) {
    panel.style.display = 'none';
    panel.innerHTML = '';
    return;
  }

  panel.style.display = 'block';

  const rows = coords.map((coord, idx) => {
    const lat = typeof coord.lat === 'number' ? coord.lat : parseFloat(coord.lat);
    const lon = typeof coord.lon === 'number' ? coord.lon : parseFloat(coord.lon);
    const elevation = elevations && typeof elevations[idx] === 'number'
      ? `${elevations[idx].toFixed(1)} m`
      : 'n/a';

    return `
      <div class="elevation-row">
        <span class="idx">#${idx}</span>
        <span class="latlon">${lat.toFixed(5)} / ${lon.toFixed(5)}</span>
        <span class="elev">${elevation}</span>
      </div>
    `;
  });

  panel.innerHTML = `
    <div class="elevation-title">Altitudes (${rows.length} points)</div>
    ${rows.join('')}
  `;
}

/**
 * Play route animation with FPV camera
 */
export function playRouteAnimation() {
  if (!scene || currentRouteCoords.length === 0) {
    console.warn("[three3d] Cannot play animation - no route loaded");
    return;
  }

  if (isAnimating) {
    console.log("[three3d] Animation already playing");
    return;
  }

  isAnimating = true;
  currentAnimationIndex = 0;

  // Disable orbit controls during FPV animation
  if (controls) {
    controls.enabled = false;
  }

  console.log("[three3d] Starting FPV animation");

  // Calculate center for coordinate conversion
  const centerLat = currentRouteCoords[Math.floor(currentRouteCoords.length / 2)].lat;
  const centerLon = currentRouteCoords[Math.floor(currentRouteCoords.length / 2)].lon;

  // Convert all coords to 3D positions
  const points = currentRouteCoords.map((coord, idx) => {
    const elevation = currentElevations[idx] || 0;
    return latLonToXYZ(coord.lat, coord.lon, elevation, centerLat, centerLon);
  });

  const totalDuration = 15000; // 15 seconds
  const startTime = Date.now();

  function animateStep() {
    if (!isAnimating) {
      return;
    }

    const elapsed = Date.now() - startTime;
    const progress = Math.min(elapsed / totalDuration, 1.0);
    currentAnimationIndex = Math.floor(progress * (points.length - 1));

    if (currentAnimationIndex < points.length - 1) {
      const currentPoint = points[currentAnimationIndex];
      const nextIndex = Math.min(currentAnimationIndex + 5, points.length - 1);
      const nextPoint = points[nextIndex];

      // Position camera at human height (1.7m / SCALE_FACTOR = 0.17)
      const eyeHeight = 0.17;
      camera.position.set(
        currentPoint.x,
        currentPoint.y + eyeHeight,
        currentPoint.z
      );

      // Look towards a point ahead on the path (not just next point)
      const lookAheadIndex = Math.min(currentAnimationIndex + 10, points.length - 1);
      const lookAtPoint = points[lookAheadIndex];
      camera.lookAt(
        lookAtPoint.x,
        lookAtPoint.y + eyeHeight,
        lookAtPoint.z
      );
    }

    if (progress < 1.0) {
      animationFrameId = requestAnimationFrame(animateStep);
    } else {
      pauseRouteAnimation();
      console.log("[three3d] Animation completed");
    }
  }

  animateStep();
}

/**
 * Pause route animation
 */
export function pauseRouteAnimation() {
  if (animationFrameId !== null) {
    cancelAnimationFrame(animationFrameId);
    animationFrameId = null;
  }
  isAnimating = false;

  // Re-enable orbit controls after FPV animation
  if (controls) {
    controls.enabled = true;
  }

  console.log("[three3d] Animation paused");
}

/**
 * Toggle 3D view visibility
 */
export function toggleThree3DView(enabled) {
  const three3dContainer = document.getElementById('three3dContainer');
  const mapContainer = document.getElementById('map');

  if (!three3dContainer || !mapContainer) {
    console.error("[three3d] Required DOM elements not found");
    return;
  }

  console.log("[three3d] Toggle view:", enabled);

  if (enabled) {
    three3dContainer.style.display = 'block';
    mapContainer.style.display = 'none';

    // Initialize if not already done
    if (!scene) {
      initThree3D();
    }

    // Re-render current route if exists
    if (currentRouteCoords.length > 0) {
      updateRoute3D(currentRouteCoords, currentElevations);
    }
  } else {
    three3dContainer.style.display = 'none';
    mapContainer.style.display = 'block';
    pauseRouteAnimation();
  }
}

// Initialize with 3D view hidden
document.addEventListener('DOMContentLoaded', () => {
  const three3dContainer = document.getElementById('three3dContainer');
  if (three3dContainer) {
    three3dContainer.style.display = 'none';
  }
});
