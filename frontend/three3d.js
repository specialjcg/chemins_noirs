// Custom 3D engine using Three.js (100% free and open-source)
// No tokens, no paid services

let scene, camera, renderer;
let routeLine;
let currentRouteCoords = [];
let currentElevations = [];
let isAnimating = false;
let animationFrameId = null;
let currentAnimationIndex = 0;

// Constants for conversion
const METERS_PER_DEGREE_LAT = 111000; // Approximate meters per degree of latitude
const SCALE_FACTOR = 1000; // Scale factor for visualization

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

  // Add lighting
  const ambientLight = new THREE.AmbientLight(0x404040, 2);
  scene.add(ambientLight);

  const directionalLight = new THREE.DirectionalLight(0xffffff, 1);
  directionalLight.position.set(100, 100, 50);
  scene.add(directionalLight);

  // Add a ground plane for reference
  const groundGeometry = new THREE.PlaneGeometry(10000, 10000);
  const groundMaterial = new THREE.MeshBasicMaterial({
    color: 0x90EE90,
    side: THREE.DoubleSide,
    transparent: true,
    opacity: 0.5
  });
  const ground = new THREE.Mesh(groundGeometry, groundMaterial);
  ground.rotation.x = -Math.PI / 2;
  scene.add(ground);

  // Handle window resize
  window.addEventListener('resize', () => {
    if (container.style.display !== 'none') {
      camera.aspect = container.clientWidth / container.clientHeight;
      camera.updateProjectionMatrix();
      renderer.setSize(container.clientWidth, container.clientHeight);
    }
  });

  console.log("[three3d] Three.js initialized successfully");
}

/**
 * Convert lat/lon/elevation to 3D coordinates
 */
function latLonToXYZ(lat, lon, elevation, centerLat, centerLon) {
  // Calculate relative position from center point
  const x = (lon - centerLon) * METERS_PER_DEGREE_LAT * Math.cos(centerLat * Math.PI / 180) / SCALE_FACTOR;
  const z = -(lat - centerLat) * METERS_PER_DEGREE_LAT / SCALE_FACTOR; // Negative to match Three.js coordinate system
  const y = elevation / SCALE_FACTOR;

  return { x, y, z };
}

/**
 * Update the 3D route visualization
 */
export function updateRoute3D(coords, elevations) {
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

  // Remove existing route
  if (routeLine) {
    scene.remove(routeLine);
    routeLine = null;
  }

  // Calculate center point for coordinate conversion
  const centerLat = currentRouteCoords[Math.floor(currentRouteCoords.length / 2)].lat;
  const centerLon = currentRouteCoords[Math.floor(currentRouteCoords.length / 2)].lon;

  // Create route line
  const points = currentRouteCoords.map((coord, idx) => {
    const elevation = currentElevations[idx] || 0;
    const pos = latLonToXYZ(coord.lat, coord.lon, elevation, centerLat, centerLon);
    return new THREE.Vector3(pos.x, pos.y, pos.z);
  });

  const geometry = new THREE.BufferGeometry().setFromPoints(points);
  const material = new THREE.LineBasicMaterial({
    color: 0xff6b35, // Bright orange
    linewidth: 3
  });
  routeLine = new THREE.Line(geometry, material);
  scene.add(routeLine);

  // Position camera to see the whole route
  if (points.length > 0) {
    const firstPoint = points[0];
    const lastPoint = points[points.length - 1];
    const midX = (firstPoint.x + lastPoint.x) / 2;
    const midZ = (firstPoint.z + lastPoint.z) / 2;
    const avgY = points.reduce((sum, p) => sum + p.y, 0) / points.length;

    camera.position.set(midX, avgY + 50, midZ + 100);
    camera.lookAt(midX, avgY, midZ);
  }

  // Render
  renderer.render(scene, camera);

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
      const nextPoint = points[currentAnimationIndex + 1];

      // Position camera at human height (1.7m = 0.0017 in scaled units)
      camera.position.set(
        currentPoint.x,
        currentPoint.y + 0.002, // Human eye height
        currentPoint.z
      );

      // Look towards next point
      camera.lookAt(
        nextPoint.x,
        nextPoint.y + 0.002,
        nextPoint.z
      );

      // Render
      renderer.render(scene, camera);
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
