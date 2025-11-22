let viewer;
let routeEntity;
let animationFrameId = null;
let currentAnimationIndex = 0;
let animationPath = [];
let isAnimating = false;
let lastAnimationTime = 0;
let animationSpeed = 1.0;

// NO TOKEN NEEDED - Using free OSM imagery and basic terrain
// For better imagery and terrain, get a free token at: https://ion.cesium.com/signup

export async function initCesiumViewer() {
  if (viewer) {
    return;
  }

  try {
    console.debug("[cesium] Initializing Cesium viewer");

    // Check if Cesium is available
    if (typeof Cesium === 'undefined') {
      console.error("[cesium] Cesium library not loaded");
      return;
    }

    // Check if container exists
    const container = document.getElementById('cesiumContainer');
    if (!container) {
      console.error("[cesium] cesiumContainer element not found");
      return;
    }

    viewer = new Cesium.Viewer('cesiumContainer', {
      baseLayerPicker: false,
      geocoder: false,
      homeButton: false,
      sceneModePicker: false,
      navigationHelpButton: false,
      animation: false,
      timeline: false,
      fullscreenButton: false,
      vrButton: false,
      // Use free OpenStreetMap imagery (no token needed)
      imageryProvider: new Cesium.OpenStreetMapImageryProvider({
        url: 'https://a.tile.openstreetmap.org/'
      })
    });

    // Use basic ellipsoid terrain (no token needed)
    // For real terrain data, you need a Cesium Ion token
    viewer.terrainProvider = new Cesium.EllipsoidTerrainProvider();

    // Configure scene for better 3D visualization
    viewer.scene.globe.enableLighting = true; // Enable sun lighting for depth
    viewer.scene.globe.depthTestAgainstTerrain = true; // Proper depth testing

    // Add exaggeration to see terrain relief better
    viewer.scene.globe.terrainExaggeration = 1.5;

    // Improve visual quality
    viewer.scene.screenSpaceCameraController.enableCollisionDetection = true;

    // Set initial camera position (Rhône-Alpes area) with better angle
    viewer.camera.setView({
      destination: Cesium.Cartesian3.fromDegrees(5.0, 45.0, 150000),
      orientation: {
        heading: 0.0,
        pitch: -Cesium.Math.toRadians(45), // More inclined view for terrain
        roll: 0.0
      }
    });

    console.debug("[cesium] Cesium viewer initialized");
  } catch (error) {
    console.error("[cesium] Failed to initialize viewer:", error);
    viewer = null;
  }
}

export function updateRoute3D(coords, elevations) {
  if (!viewer) {
    console.warn("[cesium] Viewer not initialized");
    return;
  }

  try {
    console.debug("[cesium] updateRoute3D", coords, elevations);

    // Remove existing route if any
    if (routeEntity) {
      viewer.entities.remove(routeEntity);
      routeEntity = null;
    }

    if (!Array.isArray(coords) || coords.length === 0) {
      console.debug("[cesium] No coords to display");
      return;
    }

    // Build positions array with elevations
    const positions = coords.map((coord, idx) => {
      if (!coord || typeof coord.lon !== 'number' || typeof coord.lat !== 'number') {
        console.warn("[cesium] Invalid coordinate at index", idx, coord);
        return null;
      }
      const elevation = elevations && elevations[idx] ? elevations[idx] : 0;
      // Add 50m height for better visibility above terrain
      return Cesium.Cartesian3.fromDegrees(coord.lon, coord.lat, elevation + 50);
    }).filter(pos => pos !== null);

    if (positions.length === 0) {
      console.warn("[cesium] No valid positions to display");
      return;
    }

    // Store animation path
    animationPath = positions;

    // Create polyline entity with enhanced visibility
    routeEntity = viewer.entities.add({
      polyline: {
        positions: positions,
        width: 8, // Wider line for better visibility
        material: new Cesium.PolylineOutlineMaterialProperty({
          color: Cesium.Color.fromCssColorString('#ff6b35').withAlpha(0.9), // Bright orange
          outlineWidth: 3,
          outlineColor: Cesium.Color.WHITE.withAlpha(0.8)
        }),
        clampToGround: false,
        // Add glow effect
        distanceDisplayCondition: new Cesium.DistanceDisplayCondition(0.0, 500000)
      }
    });

    // Fly to route bounds
    if (positions.length > 0) {
      viewer.flyTo(routeEntity, {
        duration: 2.0,
        offset: new Cesium.HeadingPitchRange(
          0,
          Cesium.Math.toRadians(-30),
          positions.length * 100
        )
      }).catch(error => {
        console.warn("[cesium] Failed to fly to route:", error);
      });
    }

    console.debug("[cesium] Route rendered with", positions.length, "points");
  } catch (error) {
    console.error("[cesium] Failed to update route:", error);
  }
}

/**
 * Calcule le bearing moyen sur plusieurs points pour un lissage optimal
 */
function computeSmoothedBearing(path, currentIndex, lookAhead = 10) {
  if (!path || path.length < 2) return 0;

  const bearings = [];
  const maxIndex = Math.min(currentIndex + lookAhead, path.length - 1);

  for (let i = currentIndex; i < maxIndex; i++) {
    const b = bearing(path[i], path[i + 1]);
    bearings.push(b);
  }

  if (bearings.length === 0) {
    return currentIndex > 0 ? bearing(path[currentIndex - 1], path[currentIndex]) : 0;
  }

  // Moyenne circulaire pour gérer le wrap-around (0°-360°)
  const sinSum = bearings.reduce((sum, b) => sum + Math.sin(b * Math.PI / 180), 0);
  const cosSum = bearings.reduce((sum, b) => sum + Math.cos(b * Math.PI / 180), 0);
  return Math.atan2(sinSum, cosSum) * 180 / Math.PI;
}

/**
 * Interpole deux angles avec gestion du wrap-around
 */
function interpolateBearing(current, target, factor = 0.15) {
  let diff = target - current;
  if (diff > 180) diff -= 360;
  else if (diff < -180) diff += 360;
  return (current + diff * factor + 360) % 360;
}

export function playRouteAnimation(speed = 1.0) {
  if (!viewer || animationPath.length === 0) {
    console.warn("[cesium] Cannot play animation - no route loaded");
    return;
  }

  if (isAnimating) {
    console.debug("[cesium] Animation already playing");
    return;
  }

  try {
    isAnimating = true;
    currentAnimationIndex = 0;
    lastAnimationTime = performance.now();
    let lastHeading = 0; // Pour l'interpolation du bearing

    console.debug("[cesium] Starting FPV animation at street-level with speed", speed);

    // Validate and store speed parameter
    animationSpeed = (typeof speed === 'number' && speed > 0) ? speed : 1.0;

    // Target 60 FPS = ~16.67ms per frame
    const targetFrameTime = 16.67 / animationSpeed;

    const animateFrame = (currentTime) => {
      try {
        if (!isAnimating) {
          return;
        }

        const deltaTime = currentTime - lastAnimationTime;

        // Only advance if enough time has passed for the current speed
        if (deltaTime >= targetFrameTime) {
          lastAnimationTime = currentTime;

          if (currentAnimationIndex >= animationPath.length) {
            pauseRouteAnimation();
            return;
          }

          const position = animationPath[currentAnimationIndex];
          const nextIndex = Math.min(currentAnimationIndex + 1, animationPath.length - 1);
          const nextPosition = animationPath[nextIndex];

          if (!position || !nextPosition) {
            console.warn("[cesium] Invalid position in animation path");
            pauseRouteAnimation();
            return;
          }

          // ULTRA-LOW FPV: Suivi précis de la trace au ras du sol

          // 1. Position exacte sur la trace (coordonnées géographiques)
          const currentCartographic = Cesium.Cartographic.fromCartesian(position);
          const currentLon = Cesium.Math.toDegrees(currentCartographic.longitude);
          const currentLat = Cesium.Math.toDegrees(currentCartographic.latitude);

          // 2. Altitude très basse (0.8m = hauteur des yeux d'une personne assise)
          const heightAboveGround = 0.8;
          const cameraHeight = currentCartographic.height + heightAboveGround;

          // 3. Direction précise : calculer le vecteur vers le point suivant
          const nextCartographic = Cesium.Cartographic.fromCartesian(nextPosition);
          const nextLon = Cesium.Math.toDegrees(nextCartographic.longitude);
          const nextLat = Cesium.Math.toDegrees(nextCartographic.latitude);

          // Bearing précis entre les deux points
          const targetHeading = bearing(position, nextPosition);
          const smoothedHeading = interpolateBearing(lastHeading, targetHeading, 0.3);
          lastHeading = smoothedHeading;

          // 4. MODE GPS "HEADING UP" : la carte tourne pour montrer la route vers le haut

          // Position de la caméra au-dessus du point actuel
          const cameraPosition = Cesium.Cartesian3.fromDegrees(currentLon, currentLat, cameraHeight);

          // Bearing lissé pour rotation de la carte
          const headingRad = Cesium.Math.toRadians(smoothedHeading);
          const pitchRad = Cesium.Math.toRadians(-5);  // Légèrement vers le bas

          // Utiliser setView pour un contrôle précis de la position et orientation
          viewer.camera.setView({
            destination: cameraPosition,
            orientation: {
              heading: headingRad,
              pitch: pitchRad,
              roll: 0.0
            }
          });

          // IMPORTANT: Désactiver le contrôle manuel pendant l'animation
          viewer.scene.screenSpaceCameraController.enableRotate = false;
          viewer.scene.screenSpaceCameraController.enableTranslate = false;
          viewer.scene.screenSpaceCameraController.enableZoom = false;

          currentAnimationIndex++;
        }

        // Schedule next frame
        if (isAnimating) {
          animationFrameId = requestAnimationFrame(animateFrame);
        }
      } catch (error) {
        console.error("[cesium] Error during animation frame:", error);
        pauseRouteAnimation();
      }
    };

    // Start the animation loop
    animationFrameId = requestAnimationFrame(animateFrame);
  } catch (error) {
    console.error("[cesium] Failed to start animation:", error);
    isAnimating = false;
  }
}

export function pauseRouteAnimation() {
  if (animationFrameId !== null) {
    cancelAnimationFrame(animationFrameId);
    animationFrameId = null;
  }
  isAnimating = false;

  // Re-enable camera controls when animation stops
  if (viewer) {
    viewer.scene.screenSpaceCameraController.enableRotate = true;
    viewer.scene.screenSpaceCameraController.enableTranslate = true;
    viewer.scene.screenSpaceCameraController.enableZoom = true;
  }

  console.debug("[cesium] Animation paused");
}

export function toggleCesiumView(enabled) {
  try {
    console.log("[cesium] toggleCesiumView called with enabled =", enabled);

    const cesiumContainer = document.getElementById('cesiumContainer');
    const mapContainer = document.getElementById('map');
    const mapbox3dContainer = document.getElementById('mapbox3dContainer');

    if (!cesiumContainer || !mapContainer) {
      console.error("[cesium] Required DOM elements not found");
      console.error("[cesium] cesiumContainer:", cesiumContainer);
      console.error("[cesium] mapContainer:", mapContainer);
      return;
    }

    if (enabled) {
      console.log("[cesium] Enabling Cesium view...");
      cesiumContainer.style.display = 'block';
      mapContainer.style.display = 'none';
      if (mapbox3dContainer) {
        mapbox3dContainer.style.display = 'none';
      }
      initCesiumViewer();
      console.debug("[cesium] 3D view enabled");
    } else {
      console.log("[cesium] Disabling Cesium view...");
      cesiumContainer.style.display = 'none';
      mapContainer.style.display = 'block';
      pauseRouteAnimation();
      console.debug("[cesium] 2D view enabled");
    }
  } catch (error) {
    console.error("[cesium] Failed to toggle view:", error);
  }
}

// Helper function to calculate bearing between two Cartesian3 points
function bearing(from, to) {
  const fromCartographic = Cesium.Cartographic.fromCartesian(from);
  const toCartographic = Cesium.Cartographic.fromCartesian(to);

  const lat1 = fromCartographic.latitude;
  const lat2 = toCartographic.latitude;
  const lon1 = fromCartographic.longitude;
  const lon2 = toCartographic.longitude;

  const y = Math.sin(lon2 - lon1) * Math.cos(lat2);
  const x = Math.cos(lat1) * Math.sin(lat2) -
            Math.sin(lat1) * Math.cos(lat2) * Math.cos(lon2 - lon1);

  return (Math.atan2(y, x) * 180 / Math.PI + 360) % 360;
}

// Initialize with 3D view hidden
document.addEventListener('DOMContentLoaded', () => {
  const cesiumContainer = document.getElementById('cesiumContainer');
  if (cesiumContainer) {
    cesiumContainer.style.display = 'none';
  }
});
