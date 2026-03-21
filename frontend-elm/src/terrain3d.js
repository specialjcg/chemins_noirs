/**
 * Terrain 3D Engine — Three.js renderer for orienteering game
 * Generates a 3D landscape from DEM heightmap data with paths rendered on terrain
 */

import * as THREE from 'three';

// Constants
const TILE_SIZE = 256;
const EARTH_RADIUS = 6371000;
const PLAYER_HEIGHT = 1.7; // meters
const TERRAIN_VERTICAL_SCALE = 0.3; // Reduce vertical exaggeration (0.3 = 30% of real height)
const TERRAIN_SEGMENTS = 127; // heightmap resolution
const VIEW_DISTANCE = 800; // meters visibility

let renderer, scene, camera;
let terrainMesh, pathLine, playerMarker3D;
let container;
let isRunning = false;
let playerPos = { lat: 0, lon: 0, bearing: 0 };
let terrainCenter = { lat: 0, lon: 0 };
let terrainSize = 1000; // meters
let heightData = null;
let controlPointMeshes = [];

// Convert lat/lon to local XZ coordinates (meters from terrain center)
function latLonToLocal(lat, lon) {
  const dLat = (lat - terrainCenter.lat) * Math.PI / 180;
  const dLon = (lon - terrainCenter.lon) * Math.PI / 180;
  const avgLat = terrainCenter.lat * Math.PI / 180;
  const x = dLon * EARTH_RADIUS * Math.cos(avgLat);
  const z = -dLat * EARTH_RADIUS; // negative because Z goes south in Three.js
  return { x, z };
}

// Fetch Terrarium DEM tile and decode elevation
async function fetchElevationTile(lat, lon, zoom) {
  const n = Math.pow(2, zoom);
  const x = Math.floor((lon + 180) / 360 * n);
  const y = Math.floor((1 - Math.log(Math.tan(lat * Math.PI / 180) + 1 / Math.cos(lat * Math.PI / 180)) / Math.PI) / 2 * n);

  const url = `https://s3.amazonaws.com/elevation-tiles-prod/terrarium/${zoom}/${x}/${y}.png`;

  return new Promise((resolve, reject) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      const canvas = document.createElement('canvas');
      canvas.width = TILE_SIZE;
      canvas.height = TILE_SIZE;
      const ctx = canvas.getContext('2d');
      ctx.drawImage(img, 0, 0);
      const imageData = ctx.getImageData(0, 0, TILE_SIZE, TILE_SIZE);
      const elevations = new Float32Array(TILE_SIZE * TILE_SIZE);

      for (let i = 0; i < TILE_SIZE * TILE_SIZE; i++) {
        const r = imageData.data[i * 4];
        const g = imageData.data[i * 4 + 1];
        const b = imageData.data[i * 4 + 2];
        // Terrarium encoding: elevation = (r * 256 + g + b / 256) - 32768
        elevations[i] = (r * 256 + g + b / 256) - 32768;
      }

      resolve({ elevations, tileX: x, tileY: y, zoom });
    };
    img.onerror = reject;
    img.src = url;
  });
}

// Sample elevation at a local XZ position from heightmap
function getElevationAt(localX, localZ) {
  if (!heightData) return 0;

  // Convert local position to 0-1 range within terrain
  const u = (localX / terrainSize) + 0.5;
  const v = (-localZ / terrainSize) + 0.5;

  if (u < 0 || u > 1 || v < 0 || v > 1) return 0;

  const px = Math.floor(u * (TERRAIN_SEGMENTS - 1));
  const py = Math.floor(v * (TERRAIN_SEGMENTS - 1));
  const idx = py * TERRAIN_SEGMENTS + px;

  return heightData[idx] || 0;
}

// Create the Three.js scene
function createScene(containerEl) {
  container = containerEl;
  const width = container.offsetWidth || window.innerWidth;
  const height = container.offsetHeight || window.innerHeight;

  // Renderer
  renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false });
  renderer.setSize(width, height);
  renderer.setPixelRatio(window.devicePixelRatio);
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.2;
  container.appendChild(renderer.domElement);

  // Scene
  scene = new THREE.Scene();
  scene.background = new THREE.Color(0x87CEEB); // Sky blue
  scene.fog = new THREE.Fog(0x87CEEB, VIEW_DISTANCE * 0.6, VIEW_DISTANCE);

  // Camera — first person at human height
  camera = new THREE.PerspectiveCamera(70, width / height, 0.5, VIEW_DISTANCE * 1.5);
  camera.position.set(0, PLAYER_HEIGHT, 0);

  // Lighting — natural outdoor
  const ambientLight = new THREE.AmbientLight(0x6688aa, 0.6);
  scene.add(ambientLight);

  const sunLight = new THREE.DirectionalLight(0xfff5e0, 1.2);
  sunLight.position.set(200, 300, 100);
  sunLight.castShadow = true;
  sunLight.shadow.mapSize.width = 2048;
  sunLight.shadow.mapSize.height = 2048;
  sunLight.shadow.camera.near = 10;
  sunLight.shadow.camera.far = 800;
  sunLight.shadow.camera.left = -400;
  sunLight.shadow.camera.right = 400;
  sunLight.shadow.camera.top = 400;
  sunLight.shadow.camera.bottom = -400;
  scene.add(sunLight);

  // Hemisphere light for natural sky/ground coloring
  const hemiLight = new THREE.HemisphereLight(0x87CEEB, 0x556B2F, 0.4);
  scene.add(hemiLight);

  // Block ALL click/mouse events from reaching anything below
  ['click', 'dblclick', 'contextmenu'].forEach(evt => {
    renderer.domElement.addEventListener(evt, (e) => {
      e.stopPropagation();
      e.preventDefault();
    });
  });

  // Drag to rotate view (look around)
  renderer.domElement.addEventListener('mousedown', onMouseDown);
  renderer.domElement.addEventListener('mousemove', onMouseMove);
  renderer.domElement.addEventListener('mouseup', onMouseUp);
  renderer.domElement.addEventListener('mouseleave', onMouseUp);

  // Keyboard controls handled by Elm via ports (no JS listener needed)

  // Scroll wheel to advance
  renderer.domElement.addEventListener('wheel', onWheel, { passive: false });

  // Handle resize
  window.addEventListener('resize', () => {
    const w = container.offsetWidth || window.innerWidth;
    const h = container.offsetHeight || window.innerHeight;
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
    renderer.setSize(w, h);
  });
}

// Build terrain mesh from elevation data
function buildTerrain(elevations, minElev) {
  const geometry = new THREE.PlaneGeometry(
    terrainSize, terrainSize,
    TERRAIN_SEGMENTS - 1, TERRAIN_SEGMENTS - 1
  );

  // Rotate to be horizontal (PlaneGeometry is vertical by default)
  geometry.rotateX(-Math.PI / 2);

  const positions = geometry.attributes.position.array;
  heightData = new Float32Array(TERRAIN_SEGMENTS * TERRAIN_SEGMENTS);

  for (let i = 0; i < positions.length / 3; i++) {
    const row = Math.floor(i / TERRAIN_SEGMENTS);
    const col = i % TERRAIN_SEGMENTS;

    // Sample from elevation tile
    const u = col / (TERRAIN_SEGMENTS - 1);
    const v = row / (TERRAIN_SEGMENTS - 1);
    const px = Math.floor(u * (TILE_SIZE - 1));
    const py = Math.floor(v * (TILE_SIZE - 1));
    const elevIdx = py * TILE_SIZE + px;

    const elev = ((elevations[elevIdx] || 0) - minElev) * TERRAIN_VERTICAL_SCALE;
    positions[i * 3 + 1] = elev; // Y = height (scaled)
    heightData[i] = elev;
  }

  geometry.computeVertexNormals();

  // Terrain material — bright grass green, self-lit so always visible
  const material = new THREE.MeshLambertMaterial({
    color: 0x4a7c3f,
    emissive: 0x2a4c1f,
    emissiveIntensity: 0.4,
    flatShading: false
  });

  // Apply vertex colors based on elevation for natural look
  const colors = new Float32Array(positions.length);
  const maxElev = Math.max(...heightData);
  for (let i = 0; i < positions.length / 3; i++) {
    const elev = positions[i * 3 + 1];
    const t = maxElev > 0 ? elev / maxElev : 0;

    // Green at low elevation, brown/grey at high
    const r = 0.25 + t * 0.35;
    const g = 0.45 - t * 0.15;
    const b = 0.2 + t * 0.1;
    colors[i * 3] = r;
    colors[i * 3 + 1] = g;
    colors[i * 3 + 2] = b;
  }
  geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));
  material.vertexColors = true;

  if (terrainMesh) scene.remove(terrainMesh);
  terrainMesh = new THREE.Mesh(geometry, material);
  terrainMesh.receiveShadow = true;
  scene.add(terrainMesh);
}

let roadMeshes = [];

// Build a single road/path as a flat ribbon on the ground
function buildRoad(coords, width, color) {
  if (coords.length < 2) return;

  // Convert to local coordinates
  const localPts = coords.map(c => latLonToLocal(c.lat, c.lon));

  // Build ribbon vertices (two per point: left and right of center)
  const vertices = [];
  const indices = [];

  for (let i = 0; i < localPts.length; i++) {
    const p = localPts[i];

    // Direction vector
    let dx, dz;
    if (i < localPts.length - 1) {
      dx = localPts[i + 1].x - p.x;
      dz = localPts[i + 1].z - p.z;
    } else {
      dx = p.x - localPts[i - 1].x;
      dz = p.z - localPts[i - 1].z;
    }

    // Normalize and get perpendicular
    const len = Math.sqrt(dx * dx + dz * dz);
    if (len < 0.001) continue;
    const nx = -dz / len * width * 0.5;
    const nz = dx / len * width * 0.5;

    const vi = vertices.length / 3;
    vertices.push(p.x + nx, 0.05, p.z + nz); // left
    vertices.push(p.x - nx, 0.05, p.z - nz); // right

    // Add triangle indices
    if (i > 0) {
      const prev = vi - 2;
      indices.push(prev, prev + 1, vi);
      indices.push(prev + 1, vi + 1, vi);
    }
  }

  if (vertices.length < 6) return;

  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.Float32BufferAttribute(vertices, 3));
  geo.setIndex(indices);
  geo.computeVertexNormals();

  const mat = new THREE.MeshLambertMaterial({ color, side: THREE.DoubleSide });
  const mesh = new THREE.Mesh(geo, mat);
  scene.add(mesh);
  roadMeshes.push(mesh);
}

// Build the main route path (wider, highlighted)
function buildPath(coords) {
  if (pathLine) scene.remove(pathLine);
  // Don't show the main route — it's hidden in game mode (no cheating!)
  // The player should find their own way
}

// Fetch all real roads in the area from the backend
async function fetchAndBuildRoads(lat, lon) {
  // Cover the full terrain area (~1km around center)
  const latMargin = (terrainSize / 2) / 111000;
  const lonMargin = (terrainSize / 2) / (111000 * Math.cos(lat * Math.PI / 180));

  console.log('[terrain3d] Fetching roads from backend (margin:', (latMargin * 111000).toFixed(0), 'm)...');

  try {
    const response = await fetch('/api/roads', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        min_lat: lat - latMargin,
        max_lat: lat + latMargin,
        min_lon: lon - lonMargin,
        max_lon: lon + lonMargin
      })
    });

    if (!response.ok) {
      console.warn('[terrain3d] Roads API error:', response.status);
      return;
    }

    const data = await response.json();
    console.log('[terrain3d] Got', data.count, 'road segments');

    for (const road of data.roads) {
      if (road.length >= 2) {
        buildRoad(road, 2.5, 0xC8B898); // Dirt path, 2.5m wide (easy to click)
      }
    }

    console.log('[terrain3d] Roads rendered:', roadMeshes.length);
  } catch (err) {
    console.warn('[terrain3d] Failed to fetch roads:', err);
  }
}

// Add procedural trees
function addTrees(count, areaSize) {
  const trunkGeometry = new THREE.CylinderGeometry(0.08, 0.15, 3, 5);
  const trunkMaterial = new THREE.MeshLambertMaterial({ color: 0x5C4033 });

  const leafGeometry = new THREE.SphereGeometry(1.5, 5, 4);
  const leafMaterials = [
    new THREE.MeshLambertMaterial({ color: 0x2d5a1e }),
    new THREE.MeshLambertMaterial({ color: 0x3a6b2a }),
    new THREE.MeshLambertMaterial({ color: 0x4a7c3f }),
  ];

  for (let i = 0; i < count; i++) {
    const x = (Math.random() - 0.5) * areaSize;
    const z = (Math.random() - 0.5) * areaSize;

    // Skip near player start
    if (Math.abs(x) < 5 && Math.abs(z) < 5) continue;

    const elev = 0; // Flat terrain
    const scale = 0.6 + Math.random() * 0.6;

    // Trunk
    const trunk = new THREE.Mesh(trunkGeometry, trunkMaterial);
    trunk.position.set(x, 1.5 * scale, z);
    trunk.scale.set(scale, scale, scale);
    trunk.castShadow = true;
    scene.add(trunk);

    // Leaves
    const leaves = new THREE.Mesh(leafGeometry, leafMaterials[i % 3]);
    leaves.position.set(x, 3.5 * scale, z);
    leaves.scale.set(scale, scale * (0.8 + Math.random() * 0.4), scale);
    leaves.castShadow = true;
    scene.add(leaves);
  }
}

// Add control point markers as 3D objects (orange/white poles like CO)
function addControlPoints3D(controlPoints) {
  // Remove existing
  controlPointMeshes.forEach(m => scene.remove(m));
  controlPointMeshes = [];

  // CO balise: thin pole with orange/white prism on top (like real orienteering)
  controlPoints.forEach(cp => {
    const local = latLonToLocal(cp.lat, cp.lon);

    // Thin pole
    const poleGeo = new THREE.CylinderGeometry(0.02, 0.02, 1.2, 6);
    const poleMat = new THREE.MeshLambertMaterial({ color: 0xcccccc });
    const pole = new THREE.Mesh(poleGeo, poleMat);
    pole.position.set(local.x, 0.6, local.z);
    scene.add(pole);
    controlPointMeshes.push(pole);

    // Orange/white prism (orienteering marker)
    const prismGeo = new THREE.BoxGeometry(0.3, 0.3, 0.3);
    const prismMat = new THREE.MeshLambertMaterial({ color: 0xff6600 });
    const prism = new THREE.Mesh(prismGeo, prismMat);
    prism.position.set(local.x, 1.3, local.z);
    prism.rotation.y = Math.PI / 4;
    scene.add(prism);
    controlPointMeshes.push(prism);
  });
}

// Update camera position to follow player
// Camera is ALWAYS at eye height, looking horizontally + slightly down
// Only bearing (left/right rotation) changes, never pitch
function updateCamera(lat, lon, bearing) {
  if (!camera) return;
  const local = latLonToLocal(lat, lon);

  // Position at eye height
  camera.position.set(local.x, PLAYER_HEIGHT, local.z);

  // Reset rotation manually (no lookAt which can change pitch)
  // Horizontal look direction from bearing
  const bearingRad = ((bearing % 360) * Math.PI) / 180;
  const lookDist = 50;
  const lookX = local.x + Math.sin(bearingRad) * lookDist;
  const lookZ = local.z - Math.cos(bearingRad) * lookDist;

  // Look at ground level ahead — camera at 1.7m looking at Y=0 at 50m gives natural ~2° down angle
  camera.lookAt(lookX, 0, lookZ);
}

// Click handling — raycast onto terrain, convert to lat/lon
const raycaster = new THREE.Raycaster();
const mouse = new THREE.Vector2();

// Click disabled — mouse is for rotation only
// Advance with scroll wheel or W/Z key
function onTerrainClick(event) {
  // Do nothing — rotation handled by drag
}

// Scroll wheel = advance forward in the direction the player is facing
function onWheel(e) {
  if (!isRunning || walkingInProgress) return;
  e.preventDefault();

  // Scroll down = move forward, scroll up = move backward
  const direction = e.deltaY < 0 ? 1 : -1;
  if (direction < 0) return; // Only allow forward movement

  // Calculate a point 50m ahead in the current bearing direction
  const bearingRad = playerPos.bearing * Math.PI / 180;
  const distM = 50; // meters to advance
  const dLat = Math.cos(bearingRad) * distM / 111000;
  const dLon = Math.sin(bearingRad) * distM / (111000 * Math.cos(playerPos.lat * Math.PI / 180));

  const targetLat = playerPos.lat + dLat;
  const targetLon = playerPos.lon + dLon;

  console.log('[terrain3d] Advance forward to', targetLat.toFixed(5), targetLon.toFixed(5));
  window.dispatchEvent(new CustomEvent('game-click', {
    detail: { lat: targetLat, lon: targetLon }
  }));
}

// Convert local XZ back to lat/lon
function localToLatLon(x, z) {
  const avgLat = terrainCenter.lat * Math.PI / 180;
  const lon = terrainCenter.lon + (x / (EARTH_RADIUS * Math.cos(avgLat))) * 180 / Math.PI;
  const lat = terrainCenter.lat - (z / EARTH_RADIUS) * 180 / Math.PI;
  return { lat, lon };
}

// Player rotation controls
let isDragging = false;
let lastMouseX = 0;

function onMouseDown(e) {
  if (!isRunning) return;
  e.preventDefault();
  e.stopPropagation();
  isDragging = true;
  lastMouseX = e.clientX;
}

function onMouseMove(e) {
  if (!isDragging || !isRunning) return;
  const deltaX = e.clientX - lastMouseX;
  lastMouseX = e.clientX;

  // Rotate player bearing with mouse drag
  playerPos.bearing = (playerPos.bearing + deltaX * 0.4 + 360) % 360;
  updateCamera(playerPos.lat, playerPos.lon, playerPos.bearing);

  window.dispatchEvent(new CustomEvent('game-bearing', {
    detail: { bearing: playerPos.bearing }
  }));
}

function onMouseUp() {
  isDragging = false;
}

function onKeyDown(e) {
  if (!isRunning) return;

  const rotationSpeed = 5;

  // Rotation: arrows left/right or Q/D
  if (e.key === 'ArrowLeft' || e.key === 'q' || e.key === 'Q') {
    playerPos.bearing = (playerPos.bearing - rotationSpeed + 360) % 360;
    updateCamera(playerPos.lat, playerPos.lon, playerPos.bearing);
    window.dispatchEvent(new CustomEvent('game-bearing', {
      detail: { bearing: playerPos.bearing }
    }));
    e.preventDefault();
  } else if (e.key === 'ArrowRight' || e.key === 'd' || e.key === 'D') {
    playerPos.bearing = (playerPos.bearing + rotationSpeed + 360) % 360;
    updateCamera(playerPos.lat, playerPos.lon, playerPos.bearing);
    window.dispatchEvent(new CustomEvent('game-bearing', {
      detail: { bearing: playerPos.bearing }
    }));
    e.preventDefault();
  }

  // Advance: W/Z or arrow up
  if (!walkingInProgress && (e.key === 'ArrowUp' || e.key === 'w' || e.key === 'W' || e.key === 'z' || e.key === 'Z')) {
    const bearingRad = playerPos.bearing * Math.PI / 180;
    const distM = 50;
    const dLat = Math.cos(bearingRad) * distM / 111000;
    const dLon = Math.sin(bearingRad) * distM / (111000 * Math.cos(playerPos.lat * Math.PI / 180));

    console.log('[terrain3d] Advance forward (key)');
    window.dispatchEvent(new CustomEvent('game-click', {
      detail: { lat: playerPos.lat + dLat, lon: playerPos.lon + dLon }
    }));
    e.preventDefault();
  }
}

// Animation loop
function animate() {
  if (!isRunning) return;
  requestAnimationFrame(animate);
  renderer.render(scene, camera);
}

// ============================================================
// PUBLIC API — called from main.js via ports
// ============================================================

export async function init3DWorld(lat, lon, routeCoords, controlPoints) {
  console.log('[terrain3d] Initializing 3D world at', lat, lon);

  terrainCenter = { lat, lon };

  // Create or reuse container
  container = document.getElementById('terrain3d');
  if (!container) {
    container = document.createElement('div');
    container.id = 'terrain3d';
    container.style.cssText = 'position:fixed;top:0;left:0;width:100vw;height:100vh;z-index:2;';
    document.body.appendChild(container);
  }
  container.style.display = 'block';

  // Hide MapLibre map
  const mapEl = document.getElementById('map');
  if (mapEl) mapEl.style.display = 'none';

  createScene(container);

  // Flat terrain (no DEM — cleaner, roads align perfectly)
  const flat = new Float32Array(TILE_SIZE * TILE_SIZE).fill(0);
  buildTerrain(flat, 0);

  // Fetch ALL roads from backend first
  await fetchAndBuildRoads(lat, lon);

  // Also show the route if available
  if (routeCoords && routeCoords.length >= 2) {
    buildRoad(routeCoords, 2.0, 0xC8B898);
  }

  // Add trees (avoid placing on roads)
  addTrees(400, terrainSize * 0.8);

  // Add control point markers
  if (controlPoints && controlPoints.length > 0) {
    addControlPoints3D(controlPoints);
  }

  // Initialize player position and camera
  playerPos = { lat, lon, bearing: 0 };
  updateCamera(lat, lon, 0);

  // Start rendering
  isRunning = true;
  animate();

  // Debug: expose for console inspection
  window.__t3d = { scene, camera, renderer, terrainMesh, roadMeshes, playerPos };
  console.log('[terrain3d] 3D world ready');
  console.log('[terrain3d] Camera pos:', camera.position);
  console.log('[terrain3d] Terrain:', terrainMesh?.geometry?.attributes?.position?.count, 'vertices');
  console.log('[terrain3d] Roads:', roadMeshes.length);
  console.log('[terrain3d] Scene children:', scene.children.length);
}

let walkAnimationId = null;
let walkingInProgress = false;

export function updatePlayerPosition3D(lat, lon, bearing) {
  if (!isRunning) return;
  playerPos = { lat, lon, bearing };
  updateCamera(lat, lon, bearing);
}

// Animate player walking along a path in 3D
export function walkAlongPath3D(coords) {
  if (!isRunning || coords.length < 2) return;
  if (walkingInProgress) return;
  walkingInProgress = true;

  console.log('[terrain3d] Walking along', coords.length, 'points');

  // Calculate distances
  const segments = [];
  let totalDist = 0;
  for (let i = 1; i < coords.length; i++) {
    const a = latLonToLocal(coords[i-1].lat, coords[i-1].lon);
    const b = latLonToLocal(coords[i].lat, coords[i].lon);
    const d = Math.sqrt((b.x-a.x)**2 + (b.z-a.z)**2);
    segments.push(d);
    totalDist += d;
  }

  const speed = 5.0; // m/s (jogging speed for playability)
  const durationMs = (totalDist / speed) * 1000;
  const startTime = performance.now();

  function step(now) {
    const elapsed = now - startTime;
    const progress = Math.min(elapsed / durationMs, 1.0);
    const targetDist = progress * totalDist;

    // Find position along path
    let accumulated = 0;
    let segIdx = 0;
    for (let i = 0; i < segments.length; i++) {
      if (accumulated + segments[i] >= targetDist) {
        segIdx = i;
        break;
      }
      accumulated += segments[i];
      segIdx = i;
    }

    const t = segments[segIdx] > 0 ? (targetDist - accumulated) / segments[segIdx] : 0;
    const from = coords[segIdx];
    const to = coords[Math.min(segIdx + 1, coords.length - 1)];
    const lat = from.lat + (to.lat - from.lat) * t;
    const lon = from.lon + (to.lon - from.lon) * t;

    // Calculate bearing
    const dLon = (to.lon - from.lon) * Math.PI / 180;
    const y = Math.sin(dLon) * Math.cos(to.lat * Math.PI / 180);
    const x = Math.cos(from.lat * Math.PI / 180) * Math.sin(to.lat * Math.PI / 180)
            - Math.sin(from.lat * Math.PI / 180) * Math.cos(to.lat * Math.PI / 180) * Math.cos(dLon);
    const bearing = ((Math.atan2(y, x) * 180 / Math.PI) + 360) % 360;

    // Update 3D camera
    updateCamera(lat, lon, bearing);
    playerPos = { lat, lon, bearing };

    // Send position to Elm for control point detection
    window.dispatchEvent(new CustomEvent('player-position', { detail: { lat, lon } }));
    window.dispatchEvent(new CustomEvent('game-bearing', { detail: { bearing } }));

    if (progress < 1.0) {
      walkAnimationId = requestAnimationFrame(step);
    } else {
      // Send final position
      const final = coords[coords.length - 1];
      window.dispatchEvent(new CustomEvent('player-position', { detail: { lat: final.lat, lon: final.lon } }));
      walkAnimationId = null;
      walkingInProgress = false;
      window.dispatchEvent(new CustomEvent('player-movement-done'));
    }
  }

  if (walkAnimationId) cancelAnimationFrame(walkAnimationId);
  walkAnimationId = requestAnimationFrame(step);
}

export function show3D(visible) {
  const el = document.getElementById('terrain3d');
  const mapEl = document.getElementById('map');
  if (visible) {
    if (el) el.style.display = 'block';
    if (mapEl) mapEl.style.display = 'none';
  } else {
    if (el) el.style.display = 'none';
    if (mapEl) mapEl.style.display = '';
  }
}

export function destroy3DWorld() {
  console.log('[terrain3d] Destroying 3D world');
  isRunning = false;

  if (renderer) {
    renderer.dispose();
    if (container && renderer.domElement.parentElement === container) {
      container.removeChild(renderer.domElement);
    }
  }
  if (scene) {
    scene.traverse(obj => {
      if (obj.geometry) obj.geometry.dispose();
      if (obj.material) {
        if (Array.isArray(obj.material)) obj.material.forEach(m => m.dispose());
        else obj.material.dispose();
      }
    });
  }

  const el = document.getElementById('terrain3d');
  if (el) el.style.display = 'none';

  // Show MapLibre map again
  const mapEl = document.getElementById('map');
  if (mapEl) mapEl.style.display = '';

  renderer = null;
  scene = null;
  camera = null;

  terrainMesh = null;
  pathLine = null;
  heightData = null;
  controlPointMeshes = [];
  roadMeshes = [];
  walkingInProgress = false;
  isDragging = false;
}
