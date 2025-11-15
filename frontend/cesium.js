let viewer;
let routeEntity;
let animationFrameId = null;
let currentAnimationIndex = 0;
let animationPath = [];
let isAnimating = false;
let lastAnimationTime = 0;
let animationSpeed = 1.0;

// Cesium ion access token (using default token - you can get your own at cesium.com/ion)
Cesium.Ion.defaultAccessToken = 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJqdGkiOiJlYWE1OWUxNy1mMWZiLTQzYjYtYTQ0OS1kMWFjYmFkNjc5YzciLCJpZCI6NTc3MzMsImlhdCI6MTYyNzg0NTE4Mn0.XcKpgANiY19MC4bdFUXMVEBToBmqS8kuYpUlxJHYZxk';

export function initCesiumViewer() {
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
      skyBox: false,
      skyAtmosphere: false,
      imageryProvider: new Cesium.OpenStreetMapImageryProvider({
        url: 'https://a.tile.openstreetmap.org/'
      })
    });

    // Load world terrain asynchronously
    Cesium.createWorldTerrainAsync({
      requestWaterMask: false,
      requestVertexNormals: true
    }).then(terrainProvider => {
      if (viewer) {
        viewer.terrainProvider = terrainProvider;
        console.debug("[cesium] Terrain provider loaded");
      }
    }).catch(error => {
      console.warn("[cesium] Failed to load terrain provider:", error);
    });

    // Disable fog and adjust lighting
    viewer.scene.fog.enabled = false;
    viewer.scene.globe.enableLighting = true;

    // Set initial camera position (Rhône-Alpes area)
    viewer.camera.setView({
      destination: Cesium.Cartesian3.fromDegrees(5.0, 45.0, 50000),
      orientation: {
        heading: 0.0,
        pitch: -Cesium.Math.toRadians(30),
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
      // Add 10m height for better visibility
      return Cesium.Cartesian3.fromDegrees(coord.lon, coord.lat, elevation + 10);
    }).filter(pos => pos !== null);

    if (positions.length === 0) {
      console.warn("[cesium] No valid positions to display");
      return;
    }

    // Store animation path
    animationPath = positions;

    // Create polyline entity
    routeEntity = viewer.entities.add({
      polyline: {
        positions: positions,
        width: 5,
        material: new Cesium.PolylineOutlineMaterialProperty({
          color: Cesium.Color.fromCssColorString('#4dab7b'),
          outlineWidth: 2,
          outlineColor: Cesium.Color.WHITE
        }),
        clampToGround: false
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

    console.debug("[cesium] Starting animation with speed", speed);

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

          // Calculate camera orientation
          const heading = Cesium.Math.toRadians(
            bearing(position, nextPosition)
          );
          const pitch = Cesium.Math.toRadians(-15); // Look slightly down
          const range = 200; // 200m from path

          viewer.camera.setView({
            destination: position,
            orientation: {
              heading: heading,
              pitch: pitch,
              roll: 0.0
            }
          });

          viewer.camera.moveBackward(range);

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
  console.debug("[cesium] Animation paused");
}

export function toggleCesiumView(enabled) {
  try {
    const cesiumContainer = document.getElementById('cesiumContainer');
    const mapContainer = document.getElementById('map');

    if (!cesiumContainer || !mapContainer) {
      console.error("[cesium] Required DOM elements not found");
      return;
    }

    if (enabled) {
      cesiumContainer.style.display = 'block';
      mapContainer.style.display = 'none';
      initCesiumViewer();
      console.debug("[cesium] 3D view enabled");
    } else {
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
