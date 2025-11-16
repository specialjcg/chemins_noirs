// Mapbox GL JS 3D view with terrain
// Get your own token at: https://account.mapbox.com/
mapboxgl.accessToken = 'pk.eyJ1IjoiamNnb3VsZWF1IiwiYSI6ImNtaTFuOTBtOTE5ZHkyanNmNTZwcTE1YnEifQ.zPnTKMloeQbB5_6oO1b5lA';

let map3d;
let routeSource;
let currentRouteCoords = [];
let isAnimating = false;
let animationFrameId = null;

// Helper to compute bearing between two points for drone-like camera heading
function computeBearing(from, to) {
  if (!from || !to) {
    return map3d ? map3d.getBearing() : 0;
  }
  const fromLng = from[0] * Math.PI / 180;
  const fromLat = from[1] * Math.PI / 180;
  const toLng = to[0] * Math.PI / 180;
  const toLat = to[1] * Math.PI / 180;
  const y = Math.sin(toLng - fromLng) * Math.cos(toLat);
  const x =
    Math.cos(fromLat) * Math.sin(toLat) -
    Math.sin(fromLat) * Math.cos(toLat) * Math.cos(toLng - fromLng);
  const bearing = Math.atan2(y, x) * 180 / Math.PI;
  return (bearing + 360) % 360;
}

export function initMapbox3D() {
  if (map3d) {
    return;
  }

  try {
    console.debug("[mapbox] Initializing Mapbox 3D");

    // Check if container exists
    const container = document.getElementById('mapbox3dContainer');
    if (!container) {
      console.error("[mapbox] mapbox3dContainer element not found");
      return;
    }

    // Create 3D map with terrain
    map3d = new mapboxgl.Map({
      container: 'mapbox3dContainer',
      style: 'mapbox://styles/mapbox/satellite-streets-v12', // Satellite with streets
      center: [5.0, 45.0], // Rhône-Alpes
      zoom: 8,
      pitch: 60, // Tilt for 3D view
      bearing: 0,
      antialias: true
    });

    map3d.on('load', () => {
      // Add 3D terrain
      map3d.addSource('mapbox-dem', {
        'type': 'raster-dem',
        'url': 'mapbox://mapbox.mapbox-terrain-dem-v1',
        'tileSize': 512,
        'maxzoom': 14
      });

      map3d.setTerrain({
        'source': 'mapbox-dem',
        'exaggeration': 1.5 // Exaggerate terrain for better visibility
      });

      // Add sky layer for atmosphere
      map3d.addLayer({
        'id': 'sky',
        'type': 'sky',
        'paint': {
          'sky-type': 'atmosphere',
          'sky-atmosphere-sun': [0.0, 90.0],
          'sky-atmosphere-sun-intensity': 15
        }
      });

      // Prepare route source
      map3d.addSource('route', {
        'type': 'geojson',
        'data': {
          'type': 'Feature',
          'properties': {},
          'geometry': {
            'type': 'LineString',
            'coordinates': []
          }
        }
      });

      // Add route layer
      map3d.addLayer({
        'id': 'route-layer',
        'type': 'line',
        'source': 'route',
        'layout': {
          'line-join': 'round',
          'line-cap': 'round'
        },
        'paint': {
          'line-color': '#ff6b35', // Bright orange
          'line-width': 6,
          'line-opacity': 0.9
        }
      });

      console.debug("[mapbox] Mapbox 3D initialized");
    });

  } catch (error) {
    console.error("[mapbox] Failed to initialize:", error);
    map3d = null;
  }
}

export function updateRoute3DMapbox(coords, elevations) {
  if (!map3d) {
    console.warn("[mapbox] Map not initialized");
    return;
  }

  try {
    console.debug("[mapbox] updateRoute3D", coords, elevations);

    if (!Array.isArray(coords) || coords.length === 0) {
      console.debug("[mapbox] No coords to display");
      return;
    }

    // Convert to GeoJSON coordinates [lon, lat, elevation]
    const coordinates = coords.map((coord, idx) => {
      const elevation = elevations && elevations[idx] ? elevations[idx] : 0;
      return [coord.lon, coord.lat, elevation];
    });

    // Store coordinates for animation
    currentRouteCoords = coordinates;

    // Update route source
    const source = map3d.getSource('route');
    if (source) {
      source.setData({
        'type': 'Feature',
        'properties': {},
        'geometry': {
          'type': 'LineString',
          'coordinates': coordinates
        }
      });

      // Fit bounds to route
      if (coordinates.length > 0) {
        const bounds = coordinates.reduce((bounds, coord) => {
          return bounds.extend(coord);
        }, new mapboxgl.LngLatBounds(coordinates[0], coordinates[0]));

        const initialBearing = coordinates.length > 1
          ? computeBearing(coordinates[0], coordinates[1])
          : map3d.getBearing();

        map3d.fitBounds(bounds, {
          padding: { top: 80, bottom: 80, left: 80, right: 80 },
          pitch: 65,
          bearing: initialBearing,
          duration: 2000,
          maxZoom: 15
        });
      }
    }

    console.debug("[mapbox] Route rendered with", coordinates.length, "points");
  } catch (error) {
    console.error("[mapbox] Failed to update route:", error);
  }
}

export function playRouteAnimation() {
  if (!map3d || !currentRouteCoords || currentRouteCoords.length === 0) {
    console.warn("[mapbox] Cannot play animation - no route loaded");
    return;
  }

  if (isAnimating) {
    console.debug("[mapbox] Animation already playing");
    return;
  }

  try {
    isAnimating = true;
    let currentIndex = 0;
    const totalDuration = 15000; // 15 seconds total animation
    const startTime = Date.now();

    console.debug("[mapbox] Starting animation with", currentRouteCoords.length, "points");

    function animateStep() {
      if (!isAnimating) {
        return;
      }

      const elapsed = Date.now() - startTime;
      const progress = Math.min(elapsed / totalDuration, 1.0);
      currentIndex = Math.floor(progress * (currentRouteCoords.length - 1));

      if (currentIndex < currentRouteCoords.length) {
        const coord = currentRouteCoords[currentIndex];
        const nextCoord = currentRouteCoords[Math.min(currentRouteCoords.length - 1, currentIndex + 1)];
        const bearing = computeBearing(coord, nextCoord);

        // Smoothly fly to current point with low-altitude "drone" perspective
        map3d.easeTo({
          center: [coord[0], coord[1]],
          zoom: 16.5,
          pitch: 80,
          bearing,
          duration: 120,
          easing: t => t,
          offset: [0, 60]
        });
      }

      if (progress < 1.0) {
        animationFrameId = requestAnimationFrame(animateStep);
      } else {
        pauseRouteAnimation();
        console.debug("[mapbox] Animation completed");
      }
    }

    animateStep();
  } catch (error) {
    console.error("[mapbox] Failed to start animation:", error);
    isAnimating = false;
  }
}

export function pauseRouteAnimation() {
  if (animationFrameId !== null) {
    cancelAnimationFrame(animationFrameId);
    animationFrameId = null;
  }
  isAnimating = false;
  console.debug("[mapbox] Animation paused");
}

export function toggleMapbox3DView(enabled) {
  try {
    const mapboxContainer = document.getElementById('mapbox3dContainer');
    const mapContainer = document.getElementById('map');

    if (!mapboxContainer || !mapContainer) {
      console.error("[mapbox] Required DOM elements not found");
      return;
    }

    if (enabled) {
      mapboxContainer.style.display = 'block';
      mapContainer.style.display = 'none';
      initMapbox3D();
      if (map3d) {
        map3d.resize();
      }
      console.debug("[mapbox] 3D view enabled");
    } else {
      mapboxContainer.style.display = 'none';
      mapContainer.style.display = 'block';
      pauseRouteAnimation();
      console.debug("[mapbox] 2D view enabled");
    }
  } catch (error) {
    console.error("[mapbox] Failed to toggle view:", error);
  }
}

// Initialize with 3D view hidden
document.addEventListener('DOMContentLoaded', () => {
  const mapboxContainer = document.getElementById('mapbox3dContainer');
  if (mapboxContainer) {
    mapboxContainer.style.display = 'none';
  }
});
