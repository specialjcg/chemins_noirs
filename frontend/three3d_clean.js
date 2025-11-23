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
const ELEVATION_SCALE = 5; // Stronger exaggeration for visible 3D relief

let terrainMesh = null;

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

  // Add enhanced lighting for 3D relief visibility
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.6);
  scene.add(ambientLight);

  // Main directional light (sun) from above-right
  const directionalLight = new THREE.DirectionalLight(0xffffff, 1.2);
  directionalLight.position.set(100, 150, 50);
  directionalLight.castShadow = false; // Disable shadows for performance
  scene.add(directionalLight);

  // Secondary light from opposite side for better relief definition
  const fillLight = new THREE.DirectionalLight(0xffffff, 0.4);
  fillLight.position.set(-100, 100, -50);
  scene.add(fillLight);

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
 * Fetch real terrain elevation data from Open-Meteo API
 */
async function fetchTerrainElevations(latitudes, longitudes) {
  try {
    // Open-Meteo API accepts comma-separated lists
    const latStr = latitudes.join(',');
    const lonStr = longitudes.join(',');

    const url = `https://api.open-meteo.com/v1/elevation?latitude=${latStr}&longitude=${lonStr}`;

    console.log(`[three3d] Fetching elevation data for ${latitudes.length} points...`);

    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    const data = await response.json();
    console.log(`[three3d] Received elevation data:`, data);

    return data.elevation || [];
  } catch (error) {
    console.error('[three3d] Failed to fetch elevation data:', error);
    return [];
  }
}

/**
 * Create 3D terrain mesh from route coordinates and elevations
 */
async function createTerrain(coords, elevations, centerLat, centerLon) {
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

  // Create terrain grid (moderate resolution to balance detail and API limits)
  const segments = 50; // 50x50 = 2500 points
  const geometry = new THREE.PlaneGeometry(1, 1, segments, segments);

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

  // Fetch real elevation data from API
  console.log(`[three3d] Fetching elevation for ${terrainLats.length} terrain points...`);
  const terrainElevations = await fetchTerrainElevations(terrainLats, terrainLons);

  // Apply elevations to vertices
  if (terrainElevations.length === positions.count) {
    console.log('[three3d] Applying real terrain elevations...');
    for (let i = 0; i < positions.count; i++) {
      const elevation = terrainElevations[i] || 0;
      positions.setY(i, elevation * ELEVATION_SCALE / SCALE_FACTOR);
    }
  } else {
    console.warn('[three3d] Elevation data mismatch, falling back to route-based interpolation');
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

  geometry.computeVertexNormals();
  geometry.rotateX(-Math.PI / 2); // Rotate to horizontal

  // Create material with enhanced terrain shading
  const material = new THREE.MeshStandardMaterial({
    color: 0xffffff,
    wireframe: false,
    roughness: 0.8,
    metalness: 0.2,
    flatShading: false
  });

  // Load satellite imagery from Esri World Imagery (free)
  const textureLoader = new THREE.TextureLoader();

  // Calculate tile coordinates for the center of the route
  const zoom = 14; // Higher zoom for better detail
  const centerTileX = Math.floor((centerLon + 180) / 360 * Math.pow(2, zoom));
  const centerTileY = Math.floor((1 - Math.log(Math.tan(centerLat * Math.PI / 180) + 1 / Math.cos(centerLat * Math.PI / 180)) / Math.PI) / 2 * Math.pow(2, zoom));

  // Use Esri World Imagery (same as used in map.js for consistency)
  const satelliteUrl = `https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/${zoom}/${centerTileY}/${centerTileX}`;

  console.log(`[three3d] Loading satellite texture from zoom=${zoom}, x=${centerTileX}, y=${centerTileY}`);

  // Load texture asynchronously
  textureLoader.load(
    satelliteUrl,
    (texture) => {
      texture.wrapS = THREE.RepeatWrapping;
      texture.wrapT = THREE.RepeatWrapping;
      material.map = texture;
      material.needsUpdate = true;
      console.log("[three3d] Satellite texture loaded successfully");
    },
    undefined,
    (error) => {
      console.warn("[three3d] Failed to load satellite texture, using terrain color");
      material.color.setHex(0x8B7355); // Brown terrain color
    }
  );

  return new THREE.Mesh(geometry, material);
}

/**
 * Update the 3D route visualization
 */
export async function updateRoute3D(coords, elevations) {
  if (!scene) {
    console.warn("[three3d] Scene not initialized");
    return;
  }

  console.log("[three3d] Updating route with", coords?.length, "points");

  // Store for animation
  currentRouteCoords = coords || [];
  currentElevations = elevations || [];

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

  // Calculate center point for coordinate conversion
  const centerLat = currentRouteCoords[Math.floor(currentRouteCoords.length / 2)].lat;
  const centerLon = currentRouteCoords[Math.floor(currentRouteCoords.length / 2)].lon;

  // Create 3D terrain with satellite texture and real elevation data
  console.log("[three3d] Creating terrain mesh with real elevation data...");
  terrainMesh = await createTerrain(currentRouteCoords, currentElevations, centerLat, centerLon);
  if (terrainMesh) {
    scene.add(terrainMesh);
    console.log("[three3d] Terrain added to scene");
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
