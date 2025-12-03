import maplibregl from 'maplibre-gl';

let mapInstance;
let routeSource;
let startMarker;
let endMarker;
let clickHandlerSet = false;
let terrainEnabled = false;
let bboxLayer;
let currentRoute = null;
let animationFrameId = null;
let animationStartTimestamp = null;
let routeLengthMeters = 0;
let routeDistances = [];
let animationDurationMs = 60000;
let lastBearing = null;
let lastAnimationMode = null;
let terrainSampleWarned = false;
let lastTurnAngle = 0; // Track turn angle for banking effect

// Terrain configuration - Using Terrarium format tiles from AWS
const TERRAIN_EXAGGERATION = 1.5; // Amplify terrain for better visibility
const EARTH_RADIUS_M = 6371000;

// Camera mode presets - SIGNIFICANTLY DIFFERENT for visibility
const CAMERA_MODES = {
  CINEMA: {
    name: 'Cinéma',
    icon: '🎬',
    pitch: 65,
    altitude: 12,
    zoom: 19.0,
    lookahead: 100,
    speed: 15, // m/s - moderate speed
    smoothing: 0.25,
    banking: true,
    bankingAmount: 8, // degrees
    adaptiveSpeed: true,
    minSpeed: 8, // m/s in sharp turns
    turnThreshold: 40, // degrees
    lateralOffset: 0, // No offset - follow path
    bearingOffset: 0 // Look straight ahead
  },
  LATERAL: {
    name: 'Cinéma Latéral',
    icon: '🎥',
    pitch: 55, // Gentler angle for side view
    altitude: 25, // Higher up to see more
    zoom: 18.5,
    lookahead: 150, // Look further ahead for cinematic effect
    speed: 12, // m/s - Slower for cinematic feel
    smoothing: 0.35, // Smooth movements
    banking: false, // No banking for stable shot
    bankingAmount: 0,
    adaptiveSpeed: true,
    minSpeed: 7,
    turnThreshold: 45,
    lateralOffset: 40, // 40m to the side of the path
    bearingOffset: -25 // Look towards the path (negative = look left towards path)
  },
  FPV: {
    name: 'FPV Racing',
    icon: '🏁',
    pitch: 75, // Much steeper for racing feel
    altitude: 5, // Very low
    zoom: 19.8,
    lookahead: 60, // Short lookahead for aggressive feel
    speed: 35, // m/s - FAST!
    smoothing: 0.15, // Quick reactions
    banking: true,
    bankingAmount: 18, // Aggressive banking
    adaptiveSpeed: true,
    minSpeed: 20, // Still fast in turns
    turnThreshold: 30, // More aggressive
    lateralOffset: 0,
    bearingOffset: 0
  },
  DISCOVERY: {
    name: 'Découverte',
    icon: '🔭',
    pitch: 50, // Much gentler angle
    altitude: 20, // High up for overview
    zoom: 18.3,
    lookahead: 200, // Long lookahead
    speed: 8, // m/s - Slow and contemplative
    smoothing: 0.45, // Very smooth
    banking: false,
    bankingAmount: 0,
    adaptiveSpeed: false, // Constant speed
    minSpeed: 8,
    turnThreshold: 60,
    lateralOffset: 0,
    bearingOffset: 0
  }
};

// Current camera mode (default: CINEMA for nature trails)
let currentCameraMode = CAMERA_MODES.CINEMA;

const DRONE_MIN_DURATION = 5000;
const DRONE_MAX_DURATION = 180000;

/**
 * Set 3D camera position with advanced control
 * Alternative to Free Camera API using easeTo/jumpTo
 * @param {Object} options - Camera options
 * @param {Array<number>} options.center - [lng, lat]
 * @param {number} options.zoom - Zoom level
 * @param {number} options.pitch - Camera pitch (0-85 degrees)
 * @param {number} options.bearing - Camera bearing (0-360 degrees)
 * @param {boolean} options.animate - Use animation (default: true)
 * @param {number} options.duration - Animation duration in ms (default: 1000)
 */
function setCamera3DPosition(options) {
  const {
    center,
    zoom,
    pitch = DRONE_PITCH,
    bearing = 0,
    animate = true,
    duration = 1000
  } = options;

  const cameraOptions = {
    center,
    zoom,
    pitch,
    bearing
  };

  if (animate) {
    mapInstance.easeTo({
      ...cameraOptions,
      duration,
      easing: (t) => t * (2 - t) // easeOutQuad
    });
  } else {
    mapInstance.jumpTo(cameraOptions);
  }
}

/**
 * Fly to a location with a cinematic animation
 * @param {Object} options - Flight options
 * @param {Array<number>} options.center - [lng, lat]
 * @param {number} options.zoom - Target zoom level
 * @param {number} options.pitch - Target pitch
 * @param {number} options.bearing - Target bearing
 * @param {number} options.duration - Animation duration in ms
 */
function flyToLocation(options) {
  mapInstance.flyTo({
    ...options,
    essential: true // Animation won't be interrupted by user interaction
  });
}

// Calculate bearing between two coordinates (in degrees)
function calculateBearing(start, end) {
  const startLat = start[1] * Math.PI / 180;
  const startLng = start[0] * Math.PI / 180;
  const endLat = end[1] * Math.PI / 180;
  const endLng = end[0] * Math.PI / 180;

  const dLng = endLng - startLng;

  const y = Math.sin(dLng) * Math.cos(endLat);
  const x = Math.cos(startLat) * Math.sin(endLat) -
            Math.sin(startLat) * Math.cos(endLat) * Math.cos(dLng);

  const bearing = Math.atan2(y, x) * 180 / Math.PI;
  return (bearing + 360) % 360; // Normalize to 0-360
}

function ensureMap() {
  if (mapInstance) {
    return;
  }

  mapInstance = new maplibregl.Map({
    container: 'map',
    style: {
      version: 8,
      sources: {
        'osm': {
          type: 'raster',
          tiles: ['https://tile.openstreetmap.org/{z}/{x}/{y}.png'],
          tileSize: 256,
          maxzoom: 19,
          attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors'
        },
        'satellite': {
          type: 'raster',
          tiles: ['https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}'],
          tileSize: 256,
          maxzoom: 19,
          attribution: 'Tiles &copy; Esri'
        }
      },
      layers: [
        {
          id: 'osm-tiles',
          type: 'raster',
          source: 'osm',
          minzoom: 0,
          maxzoom: 22
        }
      ],
      terrain: {
        source: 'terrainSource',
        exaggeration: TERRAIN_EXAGGERATION
      }
    },
    center: [5.0, 45.0],
    zoom: 8,
    pitch: 0,
    bearing: 0,
    antialias: true
  });

  // Add terrain source
  mapInstance.on('load', () => {
    console.log('[maplibre] Map loaded - MapLibre GL v5.x with advanced camera controls');

    // Using Terrarium format terrain tiles from AWS (free, global coverage)
    mapInstance.addSource('terrainSource', {
      type: 'raster-dem',
      tiles: ['https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png'],
      encoding: 'terrarium',
      tileSize: 256,
      maxzoom: 15
    });

    // Add hillshading for better depth perception
    mapInstance.addLayer({
      id: 'hills',
      type: 'hillshade',
      source: 'terrainSource',
      layout: { visibility: 'visible' },
      paint: { 'hillshade-shadow-color': '#473B24' }
    }, 'osm-tiles');

    // Add route source (will be populated later)
    mapInstance.addSource('route', {
      type: 'geojson',
      data: {
        type: 'FeatureCollection',
        features: []
      }
    });

    // Add route layer
    mapInstance.addLayer({
      id: 'route-line',
      type: 'line',
      source: 'route',
      layout: {
        'line-join': 'round',
        'line-cap': 'round'
      },
      paint: {
        'line-color': '#4dab7b',
        'line-width': 4
      }
    });

    console.debug('[maplibre] Map loaded with terrain support');
  });

  // Add navigation controls
  mapInstance.addControl(new maplibregl.NavigationControl({
    visualizePitch: true
  }), 'top-right');

  // Add terrain toggle control
  const terrainControl = createTerrainControl();
  mapInstance.addControl(terrainControl, 'top-right');

  // Add camera animation control
  const animationControl = createAnimationControl();
  mapInstance.addControl(animationControl, 'top-right');

  // Add camera mode control
  const cameraModeControl = createCameraModeControl();
  mapInstance.addControl(cameraModeControl, 'top-right');
}

function createTerrainControl() {
  class TerrainControl {
    onAdd(map) {
      this._map = map;
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';

      this._button = document.createElement('button');
      this._button.className = 'maplibregl-ctrl-terrain';
      this._button.textContent = '3D';
      this._button.title = 'Toggle 3D terrain';
      this._button.onclick = () => this.toggleTerrain();

      this._container.appendChild(this._button);
      return this._container;
    }

    toggleTerrain() {
      terrainEnabled = !terrainEnabled;

      if (terrainEnabled) {
        // Enable 3D terrain with realistic 45° perspective
        this._map.setTerrain({ source: 'terrainSource', exaggeration: TERRAIN_EXAGGERATION });
        this._map.easeTo({
          pitch: 45,  // Realistic viewing angle
          duration: 1000
        });
        this._button.classList.add('active');
        this._button.style.backgroundColor = '#4dab7b';
        this._button.style.color = 'white';
      } else {
        // Disable terrain and return to flat view
        this._map.setTerrain(null);
        this._map.easeTo({
          pitch: 0,
          bearing: 0,
          duration: 1000
        });
        this._button.classList.remove('active');
        this._button.style.backgroundColor = '';
        this._button.style.color = '';
      }

      console.debug('[maplibre] Terrain toggled:', terrainEnabled, '- Pitch:', terrainEnabled ? '45°' : '0°');
    }

    onRemove() {
      this._container.parentNode.removeChild(this._container);
      this._map = undefined;
    }
  }

  return new TerrainControl();
}

function createAnimationControl() {
  class AnimationControl {
    onAdd(map) {
      this._map = map;
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';

      this._button = document.createElement('button');
      this._button.className = 'maplibregl-ctrl-animation';
      this._button.innerHTML = '▶'; // Play icon
      this._button.title = 'Start/Stop camera animation';
      this._button.onclick = () => this.toggleAnimation();

      this._container.appendChild(this._button);
      return this._container;
    }

    toggleAnimation() {
      if (animationFrameId !== null) {
        // Stop animation
        stopAnimation();
        this._button.innerHTML = '▶';
        this._button.classList.remove('active');
        this._button.style.backgroundColor = '';
        this._button.style.color = '';
      } else {
        // Start animation
        startAnimation();
        this._button.innerHTML = '⏸'; // Pause icon
        this._button.classList.add('active');
        this._button.style.backgroundColor = '#4dab7b';
        this._button.style.color = 'white';
      }

      console.debug('[maplibre] Animation toggled:', animationFrameId !== null);
    }

    onRemove() {
      this._container.parentNode.removeChild(this._container);
      this._map = undefined;
    }
  }

  return new AnimationControl();
}

function createCameraModeControl() {
  class CameraModeControl {
    onAdd(map) {
      this._map = map;
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';

      this._button = document.createElement('button');
      this._button.className = 'maplibregl-ctrl-camera-mode';
      this.updateButtonDisplay();
      this._button.onclick = () => this.cycleMode();

      this._container.appendChild(this._button);
      return this._container;
    }

    updateButtonDisplay() {
      this._button.textContent = currentCameraMode.icon;
      const offsetInfo = currentCameraMode.lateralOffset !== 0 ?
        `\nOffset latéral: ${currentCameraMode.lateralOffset}m` : '';
      this._button.title = `Mode: ${currentCameraMode.name}\n` +
        `Vitesse: ${currentCameraMode.speed}m/s | ` +
        `Altitude: ${currentCameraMode.altitude}m | ` +
        `Pitch: ${currentCameraMode.pitch}°${offsetInfo}`;

      // Update visual indicator
      const modeColors = {
        'Cinéma': '#4dab7b',
        'Cinéma Latéral': '#9b59b6',
        'FPV Racing': '#ff6b35',
        'Découverte': '#4a90e2'
      };
      this._button.style.backgroundColor = modeColors[currentCameraMode.name];
      this._button.style.color = 'white';
    }

    cycleMode() {
      // Cycle through modes: CINEMA -> LATERAL -> FPV -> DISCOVERY -> CINEMA
      const modes = [CAMERA_MODES.CINEMA, CAMERA_MODES.LATERAL, CAMERA_MODES.FPV, CAMERA_MODES.DISCOVERY];
      const currentIndex = modes.indexOf(currentCameraMode);
      const nextIndex = (currentIndex + 1) % modes.length;
      const previousMode = currentCameraMode;
      currentCameraMode = modes[nextIndex];

      this.updateButtonDisplay();

      // If animation is running, recalculate metrics with new mode
      if (currentRoute && currentRoute.length > 0) {
        updateRouteMetrics(currentRoute);
        // Reset animation state for smooth transition
        lastBearing = null;
        lastTurnAngle = 0;
        lastAnimationMode = null;

        // If animation is playing, immediately apply new camera settings
        if (animationFrameId !== null) {
          const center = this._map.getCenter();
          const bearing = this._map.getBearing();
          this._map.easeTo({
            pitch: currentCameraMode.pitch,
            zoom: currentCameraMode.zoom,
            duration: 800
          });
        }
      }

      console.log(`[maplibre] 🎥 Mode: ${previousMode.name} → ${currentCameraMode.name}`);
      console.log(`  Speed: ${previousMode.speed}m/s → ${currentCameraMode.speed}m/s`);
      console.log(`  Pitch: ${previousMode.pitch}° → ${currentCameraMode.pitch}°`);
      console.log(`  Altitude: ${previousMode.altitude}m → ${currentCameraMode.altitude}m`);
      console.log(`  Banking: ${previousMode.banking ? previousMode.bankingAmount + '°' : 'OFF'} → ${currentCameraMode.banking ? currentCameraMode.bankingAmount + '°' : 'OFF'}`);
    }

    onRemove() {
      this._container.parentNode.removeChild(this._container);
      this._map = undefined;
    }
  }

  return new CameraModeControl();
}

export function initMap() {
  ensureMap();
  if (!clickHandlerSet) {
    mapInstance.on('click', (event) => {
      const { lng, lat } = event.lngLat;
      console.debug('[maplibre] click', { lat, lon: lng });
      window.dispatchEvent(
        new CustomEvent('map-click', {
          detail: { lat, lon: lng }
        })
      );
    });
    clickHandlerSet = true;
  }
}

// Center map on two markers (start and end)
export function centerOnMarkers(start, end) {
  ensureMap();

  if (!start || !end || typeof start.lat !== 'number' || typeof end.lat !== 'number') {
    console.warn('[maplibre] centerOnMarkers called with invalid coordinates');
    return;
  }

  console.debug('[maplibre] centerOnMarkers', start, end);

  // Create bounds from start and end points
  const bounds = new maplibregl.LngLatBounds(
    [start.lon, start.lat],
    [start.lon, start.lat]
  );
  bounds.extend([end.lon, end.lat]);

  // Fit map to bounds with padding
  mapInstance.fitBounds(bounds, {
    padding: 100,
    duration: 1000,
    maxZoom: 14
  });
}

export function updateRoute(coords) {
  ensureMap();
  console.debug('[maplibre] updateRoute', coords);

  if (!Array.isArray(coords) || coords.length === 0) {
    mapInstance.getSource('route').setData({
      type: 'FeatureCollection',
      features: []
    });
    currentRoute = null;
    return;
  }

  // Store route for animation
  currentRoute = coords;

  // Convert to GeoJSON LineString
  const lineString = {
    type: 'Feature',
    geometry: {
      type: 'LineString',
      coordinates: coords.map(c => [c.lon, c.lat])
    },
    properties: {}
  };

  mapInstance.getSource('route').setData({
    type: 'FeatureCollection',
    features: [lineString]
  });

  updateRouteMetrics(coords);
  animationStartTimestamp = null;
  lastBearing = null;
  lastAnimationMode = null;
  terrainSampleWarned = false;

  // Convert coords to [lon, lat] array for camera positioning
  const coordinates = coords.map(c => [c.lon, c.lat]);

  // Position camera at human eye level at start of route, looking along the path
  const startCoord = coordinates[0];
  const secondCoord = coordinates.length > 1 ? coordinates[1] : coordinates[0];

  // Calculate bearing (direction) from start to second point
  const bearing = calculateBearing(startCoord, secondCoord);

  // Get appropriate zoom level based on route length
  const bounds = coordinates.reduce((bounds, coord) => {
    return bounds.extend(coord);
  }, new maplibregl.LngLatBounds(coordinates[0], coordinates[0]));

  const ne = bounds.getNorthEast();
  const sw = bounds.getSouthWest();
  const routeDistance = Math.sqrt(
    Math.pow(ne.lng - sw.lng, 2) + Math.pow(ne.lat - sw.lat, 2)
  );

  // Zoom level: closer for shorter routes, farther for longer routes
  const baseZoom = Math.max(10, Math.min(16, 18 - Math.log2(routeDistance * 100)));

  // Position camera at current mode's height looking along the route
  const mode = currentCameraMode;
  mapInstance.easeTo({
    center: startCoord,
    zoom: mode.zoom,
    pitch: mode.pitch,
    bearing: bearing,
    duration: 2000
  });
}

export function updateSelectionMarkers(start, end) {
  ensureMap();
  console.debug('[maplibre] updateSelectionMarkers', start, end);
  updateMarker('start', start);
  updateMarker('end', end);
}

function updateMarker(type, coord) {
  let markerRef = type === 'start' ? startMarker : endMarker;

  if (coord && typeof coord.lat === 'number' && typeof coord.lon === 'number') {
    if (!markerRef) {
      const el = document.createElement('div');
      el.className = type === 'start' ? 'marker-start' : 'marker-end';
      el.style.width = '20px';
      el.style.height = '20px';
      el.style.borderRadius = '50%';
      el.style.backgroundColor = type === 'start' ? '#4CAF50' : '#F44336';
      el.style.border = '2px solid white';
      el.style.boxShadow = '0 2px 4px rgba(0,0,0,0.3)';

      markerRef = new maplibregl.Marker({ element: el })
        .setLngLat([coord.lon, coord.lat])
        .addTo(mapInstance);

      if (type === 'start') startMarker = markerRef;
      else endMarker = markerRef;
    } else {
      markerRef.setLngLat([coord.lon, coord.lat]);
    }
  } else if (markerRef) {
    markerRef.remove();
    if (type === 'start') startMarker = null;
    else endMarker = null;
  }
}

export function toggleSatelliteView(enabled) {
  ensureMap();
  console.debug('[maplibre] toggleSatelliteView', enabled);

  if (enabled) {
    mapInstance.setLayoutProperty('osm-tiles', 'visibility', 'none');

    // Add satellite layer if it doesn't exist
    if (!mapInstance.getLayer('satellite-tiles')) {
      mapInstance.addLayer({
        id: 'satellite-tiles',
        type: 'raster',
        source: 'satellite',
        minzoom: 0,
        maxzoom: 22
      }, 'hills'); // Add below hillshade
    } else {
      mapInstance.setLayoutProperty('satellite-tiles', 'visibility', 'visible');
    }
  } else {
    mapInstance.setLayoutProperty('osm-tiles', 'visibility', 'visible');
    if (mapInstance.getLayer('satellite-tiles')) {
      mapInstance.setLayoutProperty('satellite-tiles', 'visibility', 'none');
    }
  }
}

export function updateBbox(bounds) {
  ensureMap();
  console.debug('[maplibre] updateBbox', bounds);

  // Remove existing bbox layer
  if (mapInstance.getLayer('bbox-layer')) {
    mapInstance.removeLayer('bbox-layer');
  }
  if (mapInstance.getSource('bbox-source')) {
    mapInstance.removeSource('bbox-source');
  }

  // Add new bbox if bounds provided
  if (bounds && typeof bounds.min_lat === 'number') {
    const bboxGeoJSON = {
      type: 'Feature',
      geometry: {
        type: 'Polygon',
        coordinates: [[
          [bounds.min_lon, bounds.min_lat],
          [bounds.max_lon, bounds.min_lat],
          [bounds.max_lon, bounds.max_lat],
          [bounds.min_lon, bounds.max_lat],
          [bounds.min_lon, bounds.min_lat]
        ]]
      }
    };

    mapInstance.addSource('bbox-source', {
      type: 'geojson',
      data: bboxGeoJSON
    });

    mapInstance.addLayer({
      id: 'bbox-layer',
      type: 'line',
      source: 'bbox-source',
      paint: {
        'line-color': '#ff7800',
        'line-width': 2,
        'line-dasharray': [2, 2]
      }
    });

    console.debug('[maplibre] BBox rectangle added');
  }
}

// New function for 3D view toggle (replaces Three.js)
export function toggleThree3DView(enabled) {
  ensureMap();
  console.debug('[maplibre] toggleThree3DView (using Maplibre terrain)', enabled);

  // This now controls the same terrain as the 3D button
  // We'll programmatically trigger terrain with drone perspective
  if (enabled && !terrainEnabled) {
    mapInstance.setTerrain({ source: 'terrainSource', exaggeration: TERRAIN_EXAGGERATION });
    mapInstance.easeTo({ pitch: DRONE_PITCH, duration: 1000 });
    terrainEnabled = true;
  } else if (!enabled && terrainEnabled) {
    mapInstance.setTerrain(null);
    mapInstance.easeTo({ pitch: 0, bearing: 0, duration: 1000 });
    terrainEnabled = false;
  }
}

// Animation functions for camera following the route
export function startAnimation() {
  ensureMap();

  if (!currentRoute || currentRoute.length < 2) {
    console.warn('[maplibre] No route to animate');
    return;
  }

  // Enable terrain if not already enabled
  if (!terrainEnabled) {
    mapInstance.setTerrain({ source: 'terrainSource', exaggeration: TERRAIN_EXAGGERATION });
    terrainEnabled = true;
  }

  if (!routeDistances.length) {
    updateRouteMetrics(currentRoute);
  }

  console.debug('[maplibre] Starting camera animation');
  animationStartTimestamp = null;
  if (animationFrameId !== null) {
    cancelAnimationFrame(animationFrameId);
  }
  animationFrameId = requestAnimationFrame(animateCamera);
}

export function stopAnimation() {
  if (animationFrameId !== null) {
    cancelAnimationFrame(animationFrameId);
    animationFrameId = null;
    console.debug('[maplibre] Animation stopped');
  }
  animationStartTimestamp = null;
  lastBearing = null;
  lastAnimationMode = null;
  terrainSampleWarned = false;
}

function animateCamera(timestamp) {
  if (!currentRoute || currentRoute.length < 2) {
    stopAnimation();
    return;
  }

  if (animationStartTimestamp === null) {
    animationStartTimestamp = timestamp;
  }

  const mode = currentCameraMode;
  const duration = Math.max(DRONE_MIN_DURATION, animationDurationMs);
  const loopTime = (timestamp - animationStartTimestamp) % duration;
  const progress = duration === 0 ? 0 : loopTime / duration;
  const targetDistance = routeLengthMeters * progress;
  const cameraPoint = coordinateAtDistance(targetDistance);
  const lookAheadDistance = Math.min(
    targetDistance + mode.lookahead,
    routeLengthMeters
  );
  const lookAtPoint = coordinateAtDistance(lookAheadDistance);

  const pathBearing = calculateBearing(
    [cameraPoint.lon, cameraPoint.lat],
    [lookAtPoint.lon, lookAtPoint.lat]
  );

  // Apply lateral offset for cinematic side view
  let actualCameraPoint = cameraPoint;
  if (mode.lateralOffset !== 0) {
    actualCameraPoint = calculateLateralOffset(cameraPoint, pathBearing, mode.lateralOffset);
  }

  // Calculate bearing from camera to look-at point (accounting for offset)
  const targetBearing = calculateBearing(
    [actualCameraPoint.lon, actualCameraPoint.lat],
    [lookAtPoint.lon, lookAtPoint.lat]
  );

  // Apply bearing offset for angled view
  const adjustedBearing = (targetBearing + mode.bearingOffset + 360) % 360;

  // Calculate turn angle for adaptive speed and banking
  let turnAngle = 0;
  if (Number.isFinite(lastBearing)) {
    turnAngle = Math.abs(((adjustedBearing - lastBearing + 540) % 360) - 180);
  }

  const smoothedBearing = smoothAngle(lastBearing, adjustedBearing, mode.smoothing);
  lastBearing = smoothedBearing;

  // Calculate banking (roll) based on turn rate
  let roll = 0;
  if (mode.banking && Number.isFinite(lastTurnAngle)) {
    // Smooth turn angle for natural banking
    const smoothedTurnAngle = lastTurnAngle * 0.7 + turnAngle * 0.3;
    lastTurnAngle = smoothedTurnAngle;

    // Calculate roll based on turn intensity
    const turnIntensity = Math.min(smoothedTurnAngle / 90, 1); // Normalize to 0-1
    roll = turnIntensity * mode.bankingAmount;

    // Apply direction based on turn direction
    const turnDirection = ((adjustedBearing - lastBearing + 540) % 360) - 180;
    roll = turnDirection > 0 ? roll : -roll;
  } else {
    lastTurnAngle = turnAngle;
  }

  // Use jumpTo for immediate, smooth frame-by-frame camera updates
  // This eliminates the jerkiness from easeTo's 32ms animations overlapping
  if (!lastAnimationMode) {
    const offsetInfo = mode.lateralOffset !== 0 ? ` offset=${mode.lateralOffset}m` : '';
    console.debug(`[maplibre] Camera mode: ${mode.name} - pitch=${mode.pitch}° altitude=${mode.altitude}m speed=${mode.speed}m/s banking=${mode.banking}${offsetInfo}`);
    lastAnimationMode = 'jumpTo';
  }

  const cameraOptions = {
    center: [actualCameraPoint.lon, actualCameraPoint.lat],
    bearing: smoothedBearing,
    pitch: mode.pitch,
    zoom: mode.zoom
  };

  // Add roll if banking is enabled (MapLibre GL v5+ supports this)
  if (mode.banking && Math.abs(roll) > 0.5) {
    cameraOptions.roll = roll;
  }

  mapInstance.jumpTo(cameraOptions);

  animationFrameId = requestAnimationFrame(animateCamera);
}

function computeHumanZoom(lengthMeters) {
  if (!Number.isFinite(lengthMeters) || lengthMeters <= 0) {
    return HUMAN_ZOOM_MAX;
  }
  const normalized = Math.log10(Math.max(lengthMeters, 500) / 500);
  const zoom = HUMAN_ZOOM_MAX - normalized * 1.2;
  return Math.max(HUMAN_ZOOM_MIN, Math.min(HUMAN_ZOOM_MAX, zoom));
}

function updateRouteMetrics(coords) {
  routeDistances = [];
  routeLengthMeters = 0;

  if (!Array.isArray(coords) || coords.length === 0) {
    animationDurationMs = DRONE_MIN_DURATION;
    return;
  }

  const mode = currentCameraMode;
  routeDistances.push(0);

  // Calculate duration with adaptive speed if enabled
  let totalDuration = 0;

  for (let i = 0; i < coords.length - 1; i++) {
    const dist = haversineMeters(coords[i], coords[i + 1]);
    routeLengthMeters += dist;
    routeDistances.push(routeLengthMeters);

    // Calculate segment duration based on turn angle (adaptive speed)
    if (mode.adaptiveSpeed && i > 0 && i < coords.length - 2) {
      const prevPoint = [coords[i - 1].lon, coords[i - 1].lat];
      const currPoint = [coords[i].lon, coords[i].lat];
      const nextPoint = [coords[i + 1].lon, coords[i + 1].lat];

      const bearing1 = calculateBearing(prevPoint, currPoint);
      const bearing2 = calculateBearing(currPoint, nextPoint);
      const turnAngle = Math.abs(((bearing2 - bearing1 + 540) % 360) - 180);

      // Reduce speed in sharp turns
      let segmentSpeed = mode.speed;
      if (turnAngle > mode.turnThreshold) {
        const turnFactor = Math.min((turnAngle - mode.turnThreshold) / 45, 1);
        segmentSpeed = mode.speed - (mode.speed - mode.minSpeed) * turnFactor;
      }

      totalDuration += (dist / segmentSpeed) * 1000; // Convert to ms
    } else {
      totalDuration += (dist / mode.speed) * 1000;
    }
  }

  animationDurationMs = Math.min(
    DRONE_MAX_DURATION,
    Math.max(DRONE_MIN_DURATION, totalDuration)
  );
}

function coordinateAtDistance(distance) {
  if (!currentRoute || currentRoute.length === 0) {
    return { lat: 0, lon: 0 };
  }
  if (distance <= 0 || routeDistances.length === 0) {
    return currentRoute[0];
  }
  if (distance >= routeLengthMeters) {
    return currentRoute[currentRoute.length - 1];
  }

  let idx = 0;
  while (idx < routeDistances.length - 1 && routeDistances[idx + 1] < distance) {
    idx++;
  }

  const start = currentRoute[idx];
  const end = currentRoute[Math.min(idx + 1, currentRoute.length - 1)];
  const segmentStart = routeDistances[idx];
  const segmentEnd = routeDistances[Math.min(idx + 1, routeDistances.length - 1)];
  const segmentLength = Math.max(segmentEnd - segmentStart, 1e-6);
  const t = (distance - segmentStart) / segmentLength;

  return {
    lat: start.lat + (end.lat - start.lat) * t,
    lon: start.lon + (end.lon - start.lon) * t
  };
}

function haversineMeters(a, b) {
  if (!a || !b) {
    return 0;
  }
  const lat1 = a.lat * Math.PI / 180;
  const lat2 = b.lat * Math.PI / 180;
  const dLat = (b.lat - a.lat) * Math.PI / 180;
  const dLon = (b.lon - a.lon) * Math.PI / 180;

  const sinLat = Math.sin(dLat / 2);
  const sinLon = Math.sin(dLon / 2);
  const h =
    sinLat * sinLat +
    Math.cos(lat1) * Math.cos(lat2) * sinLon * sinLon;

  return 2 * EARTH_RADIUS_M * Math.asin(Math.sqrt(h));
}

/**
 * Calculate a point offset perpendicular to a bearing
 * @param {Object} point - {lat, lon}
 * @param {number} bearing - Direction in degrees
 * @param {number} offsetMeters - Distance to offset (positive = right, negative = left)
 * @returns {Object} - {lat, lon}
 */
function calculateLateralOffset(point, bearing, offsetMeters) {
  if (offsetMeters === 0) {
    return point;
  }

  // Calculate perpendicular bearing (90° to the right)
  const perpBearing = (bearing + 90) % 360;
  const perpBearingRad = perpBearing * Math.PI / 180;

  const lat1 = point.lat * Math.PI / 180;
  const lon1 = point.lon * Math.PI / 180;
  const angularDistance = offsetMeters / EARTH_RADIUS_M;

  const lat2 = Math.asin(
    Math.sin(lat1) * Math.cos(angularDistance) +
    Math.cos(lat1) * Math.sin(angularDistance) * Math.cos(perpBearingRad)
  );

  const lon2 = lon1 + Math.atan2(
    Math.sin(perpBearingRad) * Math.sin(angularDistance) * Math.cos(lat1),
    Math.cos(angularDistance) - Math.sin(lat1) * Math.sin(lat2)
  );

  return {
    lat: lat2 * 180 / Math.PI,
    lon: lon2 * 180 / Math.PI
  };
}

/**
 * Smooth angle interpolation with spline-like easing
 * Uses a smooth hermite interpolation for more natural camera rotation
 */
function smoothAngle(previous, target, factor) {
  if (!Number.isFinite(previous)) {
    return target;
  }
  let delta = ((target - previous + 540) % 360) - 180;

  // Apply smooth hermite easing (similar to Catmull-Rom spline)
  // This creates a more natural, rounded transition
  const easedFactor = factor * factor * (3 - 2 * factor); // smoothstep

  return previous + delta * easedFactor;
}

function queryElevation(point) {
  if (
    !point ||
    typeof mapInstance?.queryTerrainElevation !== 'function'
  ) {
    return 0;
  }
  const elevation = mapInstance.queryTerrainElevation(
    [point.lon, point.lat],
    { exaggerated: false }
  );
  return Number.isFinite(elevation) ? elevation : 0;
}
