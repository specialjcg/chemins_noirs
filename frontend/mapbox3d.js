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

/**
 * Pure function: Calcule le bearing moyen sur un segment de route pour un lissage optimal
 * @param {Array<[number, number]>} coords - Coordonnées de la route
 * @param {number} currentIndex - Index actuel
 * @param {number} lookAhead - Nombre de points à regarder en avant (default: 5)
 * @returns {number} Bearing lissé en degrés
 */
function computeSmoothedBearing(coords, currentIndex, lookAhead = 5) {
  if (!coords || coords.length < 2) {
    return 0;
  }

  // Calculer les bearings des segments suivants
  const bearings = [];
  const maxIndex = Math.min(currentIndex + lookAhead, coords.length - 1);

  for (let i = currentIndex; i < maxIndex; i++) {
    const bearing = computeBearing(coords[i], coords[i + 1]);
    bearings.push(bearing);
  }

  // Cas particulier: gérer le passage de 359° à 0° (wrap around)
  // Convertir en vecteurs puis moyenner pour éviter les sauts
  if (bearings.length === 0) {
    return computeBearing(
      coords[Math.max(0, currentIndex - 1)],
      coords[currentIndex]
    );
  }

  // Moyenne circulaire pour gérer le wrap-around (0°-360°)
  const sinSum = bearings.reduce((sum, b) => sum + Math.sin(b * Math.PI / 180), 0);
  const cosSum = bearings.reduce((sum, b) => sum + Math.cos(b * Math.PI / 180), 0);
  const avgBearing = Math.atan2(sinSum, cosSum) * 180 / Math.PI;

  return (avgBearing + 360) % 360;
}

/**
 * Pure function: Interpole deux angles avec gestion du wrap-around
 * @param {number} current - Angle actuel en degrés
 * @param {number} target - Angle cible en degrés
 * @param {number} factor - Facteur d'interpolation (0-1)
 * @returns {number} Angle interpolé
 */
function interpolateBearing(current, target, factor = 0.3) {
  // Gérer le plus court chemin entre les angles
  let diff = target - current;

  if (diff > 180) {
    diff -= 360;
  } else if (diff < -180) {
    diff += 360;
  }

  return (current + diff * factor + 360) % 360;
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
    let lastBearing = map3d.getBearing(); // Bearing initial pour l'interpolation

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

        // Calculer le bearing lissé en regardant plus loin (12 points) pour plus de fluidité
        const targetBearing = computeSmoothedBearing(currentRouteCoords, currentIndex, 12);

        // Interpolation plus douce (0.2 au lieu de 0.25) pour transitions ultra-fluides
        const smoothedBearing = interpolateBearing(lastBearing, targetBearing, 0.2);
        lastBearing = smoothedBearing; // Mémoriser pour la prochaine frame

        // Vue drone immersive : très proche de la route avec transition fluide
        map3d.easeTo({
          center: [coord[0], coord[1]],
          zoom: 17.5,       // Plus proche qu'avant (16.5 → 17.5)
          pitch: 75,        // Légèrement moins vertical (80 → 75) pour mieux voir la route
          bearing: smoothedBearing,
          duration: 250,    // Transitions encore plus longues pour fluidité maximale
          easing: t => t < 0.5 ? 2 * t * t : -1 + (4 - 2 * t) * t, // Easing quadratique
          offset: [0, 80]   // Caméra légèrement plus haute sur l'écran
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
