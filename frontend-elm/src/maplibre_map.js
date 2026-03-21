import maplibregl from 'maplibre-gl';
import mlcontour from 'maplibre-contour';

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
let lastTerrainZoomAdjust = 0; // Smoothed zoom offset for terrain avoidance
let waypointMarkers = []; // Markers for multi-point route waypoints
let currentMapStyle = 'topo'; // 'topo' | 'satellite' | 'hybrid'
let kmMarkers = []; // Kilometer markers along route
let poiMarkers = []; // POI markers on map
let poisVisible = false; // POI toggle state
let lastPoiBbox = null; // Last fetched POI bbox to avoid re-fetching

// Terrain configuration - Using Terrarium format tiles from AWS
const TERRAIN_EXAGGERATION = 1.5; // Amplify terrain for better visibility
const EARTH_RADIUS_M = 6371000;
const DRONE_PITCH = 60; // Default pitch for 3D drone view

// Camera mode presets - SIGNIFICANTLY DIFFERENT for visibility
const CAMERA_MODES = {
  CINEMA: {
    name: 'Cinéma',
    icon: '🎬',
    pitch: 65,
    altitude: 12,
    zoom: 19.0,
    lookahead: 100,
    speed: 60, // m/s - fast
    smoothing: 0.25,
    banking: true,
    bankingAmount: 8, // degrees
    adaptiveSpeed: true,
    minSpeed: 30, // m/s in sharp turns
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
    speed: 50, // m/s - fast cinematic
    smoothing: 0.35, // Smooth movements
    banking: false, // No banking for stable shot
    bankingAmount: 0,
    adaptiveSpeed: true,
    minSpeed: 24,
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
    speed: 70, // m/s - FAST!
    smoothing: 0.15, // Quick reactions
    banking: true,
    bankingAmount: 18, // Aggressive banking
    adaptiveSpeed: true,
    minSpeed: 40, // Still fast in turns
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
    speed: 36, // m/s - fast discovery
    smoothing: 0.45, // Very smooth
    banking: false,
    bankingAmount: 0,
    adaptiveSpeed: false, // Constant speed
    minSpeed: 8,
    turnThreshold: 60,
    lateralOffset: 0,
    bearingOffset: 0
  },
  WALKING: {
    name: 'Marche',
    icon: '🥾',
    pitch: 70,
    altitude: 3,
    zoom: 17,
    lookahead: 30,
    speed: 3, // m/s ~10 km/h (accelerated walking)
    smoothing: 0.3,
    banking: false,
    bankingAmount: 0,
    adaptiveSpeed: false,
    minSpeed: 3,
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

  window.__mapInstance = null; // Debug: expose map instance globally
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
          tiles: [
            'https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}'
          ],
          tileSize: 256,
          maxzoom: 19,
          attribution: '© Esri'
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
      ]
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
    window.__mapInstance = mapInstance;

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

    // Generate contour lines on-the-fly from DEM tiles
    const demSource = new mlcontour.DemSource({
      url: 'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png',
      encoding: 'terrarium',
      maxzoom: 13,
      worker: true
    });
    demSource.setupMaplibre(maplibregl);

    mapInstance.addSource('contours', {
      type: 'vector',
      tiles: [
        demSource.contourProtocolUrl({
          multiplier: 1,
          overzoom: 1,
          thresholds: {
            11: [200],
            12: [100],
            13: [50],
            14: [20]
          },
          elevationKey: 'ele',
          levelKey: 'level',
          contourLayer: 'contour'
        })
      ],
      maxzoom: 15
    });

    // Contour lines — minor (level 0) thin, major (level 1) thicker
    mapInstance.addLayer({
      id: 'contour-lines',
      type: 'line',
      source: 'contours',
      'source-layer': 'contour',
      layout: { visibility: 'visible' },
      paint: {
        'line-color': 'rgba(120, 90, 50, 0.35)',
        'line-width': ['match', ['get', 'level'], 1, 1.2, 0.5]
      },
      minzoom: 11
    });

    // Contour labels — elevation text on major lines
    mapInstance.addLayer({
      id: 'contour-labels',
      type: 'symbol',
      source: 'contours',
      'source-layer': 'contour',
      filter: ['>', ['get', 'level'], 0],
      layout: {
        visibility: 'visible',
        'symbol-placement': 'line',
        'text-field': ['concat', ['number-format', ['get', 'ele'], {}], ' m'],
        'text-font': ['Noto Sans Regular'],
        'text-size': 10,
        'text-max-angle': 25,
        'text-padding': 5
      },
      paint: {
        'text-color': 'rgba(90, 65, 30, 0.7)',
        'text-halo-color': 'rgba(255, 255, 255, 0.8)',
        'text-halo-width': 1.5
      },
      minzoom: 13
    });

    // Add route source (will be populated later)
    mapInstance.addSource('route', {
      type: 'geojson',
      data: {
        type: 'FeatureCollection',
        features: []
      }
    });

    // Add route layer with white outline for better visibility on satellite
    // Add outline layer first (drawn under the main line)
    mapInstance.addLayer({
      id: 'route-line-outline',
      type: 'line',
      source: 'route',
      layout: {
        'line-join': 'round',
        'line-cap': 'round'
      },
      paint: {
        'line-color': '#ffffff',
        'line-width': 6,
        'line-opacity': 0.8
      }
    });

    // Add main route layer on top
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

  // Add pitch toggle control (for free tilt)
  const pitchControl = createPitchControl();
  mapInstance.addControl(pitchControl, 'top-right');

  // Add POI toggle control
  const poiControl = createPoiControl();
  mapInstance.addControl(poiControl, 'top-right');

  // Add map style switcher (topo / satellite / hybrid)
  const styleControl = createStyleSwitcherControl();
  mapInstance.addControl(styleControl, 'bottom-left');
}

function createStyleSwitcherControl() {
  const STYLES = [
    { id: 'topo',      label: 'Topo',      icon: '◈' },
    { id: 'satellite', label: 'Satellite',  icon: '◉' },
    { id: 'hybrid',    label: 'Hybride',    icon: '◎' }
  ];

  class StyleSwitcherControl {
    onAdd(map) {
      this._map = map;
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl style-switcher';

      STYLES.forEach(s => {
        const btn = document.createElement('button');
        btn.className = 'style-switcher-btn' + (s.id === currentMapStyle ? ' active' : '');
        btn.dataset.style = s.id;
        btn.title = s.label;
        btn.innerHTML = `<span class="style-icon">${s.icon}</span><span class="style-label">${s.label}</span>`;
        btn.onclick = () => {
          switchMapStyle(s.id);
          this._container.querySelectorAll('.style-switcher-btn').forEach(b => b.classList.remove('active'));
          btn.classList.add('active');
        };
        this._container.appendChild(btn);
      });

      return this._container;
    }

    onRemove() {
      this._container.parentNode.removeChild(this._container);
      this._map = undefined;
    }
  }
  return new StyleSwitcherControl();
}

function createPitchControl() {
  class PitchControl {
    onAdd(map) {
      this._map = map;
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';

      this._button = document.createElement('button');
      this._button.className = 'maplibregl-ctrl-pitch';
      this._button.textContent = '45°';
      this._button.title = 'Toggle 45° pitch';
      this._button.onclick = () => this.togglePitch();

      this._container.appendChild(this._button);
      return this._container;
    }

    togglePitch() {
      const currentPitch = this._map.getPitch();
      const targetPitch = currentPitch < 10 ? 45 : 0;
      this._map.easeTo({ pitch: targetPitch, duration: 800 });
      this._button.textContent = targetPitch === 0 ? '45°' : '0°';
    }

    onRemove() {
      this._container.parentNode.removeChild(this._container);
      this._map = undefined;
    }
  }
  return new PitchControl();
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

function createPoiControl() {
  const POI_ICONS = {
    water: '💧', peak: '⛰️', hut: '🏠', shelter: '🏕️',
    parking: '🅿️', viewpoint: '👁️', saddle: '🏔️'
  };

  class PoiControl {
    onAdd(map) {
      this._map = map;
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';

      this._button = document.createElement('button');
      this._button.className = 'maplibregl-ctrl-poi';
      this._button.textContent = 'POI';
      this._button.title = 'Afficher les points d\'intérêt';
      this._button.onclick = () => this.togglePoi();

      this._container.appendChild(this._button);
      return this._container;
    }

    togglePoi() {
      poisVisible = !poisVisible;

      if (poisVisible) {
        this._button.style.backgroundColor = '#4dab7b';
        this._button.style.color = 'white';
        this.fetchPois();
        // Refresh POIs when map moves
        this._moveHandler = () => this.fetchPois();
        this._map.on('moveend', this._moveHandler);
      } else {
        this._button.style.backgroundColor = '';
        this._button.style.color = '';
        clearPoiMarkers();
        if (this._moveHandler) {
          this._map.off('moveend', this._moveHandler);
        }
      }
    }

    fetchPois() {
      const bounds = this._map.getBounds();
      const bbox = {
        min_lat: bounds.getSouth(),
        max_lat: bounds.getNorth(),
        min_lon: bounds.getWest(),
        max_lon: bounds.getEast()
      };

      // Skip if same bbox (throttle)
      const bboxKey = `${bbox.min_lat.toFixed(3)},${bbox.max_lat.toFixed(3)},${bbox.min_lon.toFixed(3)},${bbox.max_lon.toFixed(3)}`;
      if (bboxKey === lastPoiBbox) return;
      lastPoiBbox = bboxKey;

      const url = `/api/pois?min_lat=${bbox.min_lat}&max_lat=${bbox.max_lat}&min_lon=${bbox.min_lon}&max_lon=${bbox.max_lon}`;
      fetch(url)
        .then(r => r.json())
        .then(pois => {
          clearPoiMarkers();
          pois.forEach(poi => {
            const icon = POI_ICONS[poi.poi_type] || '📍';
            const el = document.createElement('div');
            el.style.fontSize = '18px';
            el.style.cursor = 'pointer';
            el.style.filter = 'drop-shadow(0 1px 2px rgba(0,0,0,0.5))';
            el.textContent = icon;

            const popup = new maplibregl.Popup({ offset: 15, closeButton: false })
              .setText(poi.name || poi.poi_type);

            const marker = new maplibregl.Marker({ element: el, anchor: 'center' })
              .setLngLat([poi.lon, poi.lat])
              .setPopup(popup)
              .addTo(mapInstance);

            poiMarkers.push(marker);
          });
          console.debug(`[maplibre] Displayed ${pois.length} POIs`);
        })
        .catch(err => console.warn('[maplibre] POI fetch error:', err));
    }

    onRemove() {
      this._container.parentNode.removeChild(this._container);
      if (this._moveHandler) {
        this._map.off('moveend', this._moveHandler);
      }
      this._map = undefined;
    }
  }
  return new PoiControl();
}

function clearPoiMarkers() {
  poiMarkers.forEach(m => m.remove());
  poiMarkers = [];
}

export function initMap() {
  ensureMap();
  if (!clickHandlerSet) {
    mapInstance.on('click', (event) => {
      if (gameMode) return; // Game mode handles clicks separately
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

  const routeSource = mapInstance.getSource('route');

  if (!Array.isArray(coords) || coords.length === 0) {
    if (routeSource) {
      routeSource.setData({ type: 'FeatureCollection', features: [] });
    }
    currentRoute = null;
    // Clear km markers when route is cleared
    kmMarkers.forEach(m => m.remove());
    kmMarkers = [];
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

  if (routeSource) {
    routeSource.setData({ type: 'FeatureCollection', features: [lineString] });
  }

  updateRouteMetrics(coords);
  animationStartTimestamp = null;
  lastBearing = null;
  lastAnimationMode = null;
  terrainSampleWarned = false;

  // Place kilometer markers along the route
  updateKmMarkers(coords);
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
  // Legacy compatibility: true → satellite, false → topo
  switchMapStyle(enabled ? 'satellite' : 'topo');
}

/**
 * Switch map style between topo, satellite, and hybrid.
 * @param {string} style - 'topo' | 'satellite' | 'hybrid'
 */
export function switchMapStyle(style) {
  ensureMap();
  currentMapStyle = style;
  console.debug('[maplibre] switchMapStyle', style);

  // Ensure satellite layer exists — insert it below osm-tiles so stacking is correct
  if (!mapInstance.getLayer('satellite-tiles') && (style === 'satellite' || style === 'hybrid')) {
    mapInstance.addLayer({
      id: 'satellite-tiles',
      type: 'raster',
      source: 'satellite',
      minzoom: 0,
      maxzoom: 22
    }, 'osm-tiles'); // Insert below OSM so hybrid overlay works
  }

  const showOsm = style === 'topo' || style === 'hybrid';
  const showSatellite = style === 'satellite' || style === 'hybrid';

  // Set satellite visibility
  if (mapInstance.getLayer('satellite-tiles')) {
    mapInstance.setLayoutProperty('satellite-tiles', 'visibility', showSatellite ? 'visible' : 'none');
  }

  // Set OSM visibility and opacity
  if (mapInstance.getLayer('osm-tiles')) {
    mapInstance.setLayoutProperty('osm-tiles', 'visibility', showOsm ? 'visible' : 'none');
    // In hybrid mode, make OSM semi-transparent on top of satellite
    mapInstance.setPaintProperty('osm-tiles', 'raster-opacity', style === 'hybrid' ? 0.45 : 1);
  }

  // Set hillshade visibility — hide in pure satellite, show in topo/hybrid
  if (mapInstance.getLayer('hills')) {
    mapInstance.setLayoutProperty('hills', 'visibility', style === 'satellite' ? 'none' : 'visible');
  }

  // Set contour lines visibility — show in topo and 3D, hide in pure satellite
  const showContours = style !== 'satellite';
  if (mapInstance.getLayer('contour-lines')) {
    mapInstance.setLayoutProperty('contour-lines', 'visibility', showContours ? 'visible' : 'none');
  }
  if (mapInstance.getLayer('contour-labels')) {
    mapInstance.setLayoutProperty('contour-labels', 'visibility', showContours ? 'visible' : 'none');
  }

  // Adjust terrain exaggeration for satellite mode (more dramatic relief)
  // Skip terrain in game mode (causes blurry tiles in 2D view)
  if (!gameMode && (terrainEnabled || mapInstance.getTerrain())) {
    const exag = style === 'satellite' ? 1.8 : TERRAIN_EXAGGERATION;
    mapInstance.setTerrain({ source: 'terrainSource', exaggeration: exag });
  }

  // Ensure route layers are always on top
  ensureRouteLayers();

  // Sync the on-map style switcher buttons
  document.querySelectorAll('.style-switcher-btn').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.style === style);
  });
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
  lastTerrainZoomAdjust = 0;
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

/**
 * Project a point onto the nearest position on a polyline.
 * Returns the projected {lat, lon} that lies exactly on the line.
 */
function snapToPolyline(point, polyline) {
  let bestDist = Infinity;
  let bestPoint = point;

  for (let i = 0; i < polyline.length - 1; i++) {
    const a = polyline[i];
    const b = polyline[i + 1];

    // Vector AB and AP
    const abLat = b.lat - a.lat;
    const abLon = b.lon - a.lon;
    const apLat = point.lat - a.lat;
    const apLon = point.lon - a.lon;

    // Project: t = dot(AP, AB) / dot(AB, AB), clamped to [0, 1]
    const dotAB = abLat * abLat + abLon * abLon;
    if (dotAB < 1e-14) continue; // degenerate segment
    const t = Math.max(0, Math.min(1, (apLat * abLat + apLon * abLon) / dotAB));

    const projLat = a.lat + t * abLat;
    const projLon = a.lon + t * abLon;

    const dLat = point.lat - projLat;
    const dLon = point.lon - projLon;
    const dist = dLat * dLat + dLon * dLon;

    if (dist < bestDist) {
      bestDist = dist;
      bestPoint = { lat: projLat, lon: projLon };
    }
  }
  return bestPoint;
}

export function updateWaypointMarkers(waypoints) {
  // Remove all existing waypoint markers
  waypointMarkers.forEach(marker => marker.remove());
  waypointMarkers = [];

  if (!Array.isArray(waypoints) || !mapInstance) {
    return;
  }

  // Create numbered, draggable markers with delete button
  // If a route is displayed, snap markers onto the route line
  waypoints.forEach((coord, index) => {
    // The coord may already be a backend-snapped position (on the road).
    // Apply JS snap as safety net to ensure marker sits exactly on the route line.
    const hasRoute = currentRoute && currentRoute.length >= 2;
    let displayCoord = coord;
    if (hasRoute) {
      const snapped = snapToPolyline(coord, currentRoute);
      const dLat = (snapped.lat - coord.lat) * 111000;
      const dLon = (snapped.lon - coord.lon) * 111000 * Math.cos(coord.lat * Math.PI / 180);
      const distM = Math.sqrt(dLat * dLat + dLon * dLon);
      console.log(`[snap] wp${index + 1}: (${coord.lat.toFixed(6)},${coord.lon.toFixed(6)}) → (${snapped.lat.toFixed(6)},${snapped.lon.toFixed(6)}) Δ${distM.toFixed(1)}m ${distM < 50 ? '✓' : '✗ >50m'}`);
      if (distM < 50) {
        displayCoord = snapped;
      }
    }
    // Circle element = marker root (no wrapper div to avoid anchor miscalculation)
    const el = document.createElement('div');
    el.style.boxSizing = 'border-box';
    el.style.width = '28px';
    el.style.height = '28px';
    el.style.borderRadius = '50%';
    el.style.backgroundColor = '#2196F3';
    el.style.border = '2px solid white';
    el.style.display = 'flex';
    el.style.alignItems = 'center';
    el.style.justifyContent = 'center';
    el.style.fontSize = '12px';
    el.style.fontWeight = 'bold';
    el.style.color = 'white';
    el.style.cursor = 'grab';
    el.style.boxShadow = '0 2px 6px rgba(0,0,0,0.3)';
    el.textContent = (index + 1).toString();

    // Delete button (× top-right, visible on hover)
    const delBtn = document.createElement('div');
    delBtn.style.position = 'absolute';
    delBtn.style.top = '-8px';
    delBtn.style.right = '-8px';
    delBtn.style.width = '18px';
    delBtn.style.height = '18px';
    delBtn.style.borderRadius = '50%';
    delBtn.style.backgroundColor = '#F44336';
    delBtn.style.border = '1.5px solid white';
    delBtn.style.display = 'none';
    delBtn.style.alignItems = 'center';
    delBtn.style.justifyContent = 'center';
    delBtn.style.fontSize = '11px';
    delBtn.style.fontWeight = 'bold';
    delBtn.style.color = 'white';
    delBtn.style.cursor = 'pointer';
    delBtn.style.lineHeight = '1';
    delBtn.textContent = '×';
    el.appendChild(delBtn);

    // Show/hide delete button on hover
    el.addEventListener('mouseenter', () => { delBtn.style.display = 'flex'; });
    el.addEventListener('mouseleave', () => { delBtn.style.display = 'none'; });

    // Delete button click → dispatch event to Elm
    delBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      window.dispatchEvent(new CustomEvent('waypoint-deleted', { detail: { index } }));
    });

    const marker = new maplibregl.Marker({ element: el, draggable: true, anchor: 'center' })
      .setLngLat([displayCoord.lon, displayCoord.lat])
      .addTo(mapInstance);

    // Drag end → dispatch event to Elm with new position
    marker.on('dragend', () => {
      const lngLat = marker.getLngLat();
      window.dispatchEvent(new CustomEvent('waypoint-dragged', {
        detail: { index, lat: lngLat.lat, lon: lngLat.lng }
      }));
    });

    waypointMarkers.push(marker);
  });

  console.debug(`[maplibre] Updated ${waypoints.length} draggable waypoint markers`);
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

  // Adjust zoom when terrain is enabled to prevent camera going underground.
  // Higher terrain → lower zoom (camera further away) to stay above surface.
  let adjustedZoom = mode.zoom;
  if (terrainEnabled) {
    const groundElevation = queryElevation(actualCameraPoint);
    // Compute needed zoom reduction: each meter of exaggerated elevation
    // needs proportional zoom-out. Factor tuned for pitch 55-75° views.
    const targetAdjust = Math.max(0, groundElevation * TERRAIN_EXAGGERATION * 0.002);
    // Smooth the adjustment to avoid jerky zoom on terrain transitions
    lastTerrainZoomAdjust = lastTerrainZoomAdjust * 0.92 + targetAdjust * 0.08;
    adjustedZoom -= lastTerrainZoomAdjust;
  }

  const cameraOptions = {
    center: [actualCameraPoint.lon, actualCameraPoint.lat],
    bearing: smoothedBearing,
    pitch: mode.pitch,
    zoom: adjustedZoom
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

/** Place small numbered markers every kilometer along the route polyline */
function updateKmMarkers(coords) {
  // Remove existing km markers
  kmMarkers.forEach(m => m.remove());
  kmMarkers = [];

  if (!coords || coords.length < 2 || !mapInstance) return;

  let accDist = 0;
  let nextKm = 1;

  for (let i = 1; i < coords.length; i++) {
    const segDist = haversineMeters(coords[i - 1], coords[i]) / 1000; // km
    const prevDist = accDist;
    accDist += segDist;

    // Check if we crossed a km boundary in this segment
    while (accDist >= nextKm) {
      const t = (nextKm - prevDist) / segDist;
      const lat = coords[i - 1].lat + (coords[i].lat - coords[i - 1].lat) * t;
      const lon = coords[i - 1].lon + (coords[i].lon - coords[i - 1].lon) * t;

      const el = document.createElement('div');
      el.className = 'km-marker';
      el.textContent = nextKm + 'km';

      const marker = new maplibregl.Marker({ element: el, anchor: 'center' })
        .setLngLat([lon, lat])
        .addTo(mapInstance);

      kmMarkers.push(marker);
      nextKm++;
    }
  }

  console.debug(`[maplibre] Placed ${kmMarkers.length} km markers`);
}

/** Ensure route layers are on top of all raster layers */
function ensureRouteLayers() {
  if (mapInstance.getLayer('route-line-outline')) {
    mapInstance.moveLayer('route-line-outline');
  }
  if (mapInstance.getLayer('route-line')) {
    mapInstance.moveLayer('route-line');
  }
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

// ============================================================
// ORIENTEERING GAME - First-person camera & movement
// ============================================================

let gameMode = false;
let savedCameraState = null;
let playerMarker = null;
let controlPointMarkers = [];
let moveAnimationId = null;
let gameTimerInterval = null;

export function enterFirstPersonMode(lat, lon, bearing) {
  if (!mapInstance || !currentRoute || currentRoute.length < 2) {
    console.warn('[game] No route to simulate');
    return;
  }
  console.log('[game] Starting route simulation at', lat, lon);

  gameMode = true;

  // Add game-mode class to body
  document.body.classList.add('game-mode');

  // Force Elm HUD above map
  setTimeout(() => {
    let el = document.querySelector('.game-hud');
    while (el && el !== document.body) {
      el.style.position = el.classList.contains('game-hud') ? 'fixed' : 'fixed';
      el.style.zIndex = '999999';
      el.style.pointerEvents = 'none';
      el.style.background = 'transparent';
      el.style.top = '0';
      el.style.left = '0';
      el.style.width = '100vw';
      el.style.height = '100vh';
      el = el.parentElement;
    }
    const map = document.getElementById('map');
    if (map) map.style.zIndex = '1';
  }, 50);

  // Save camera state for restoring later
  savedCameraState = {
    center: mapInstance.getCenter(),
    zoom: mapInstance.getZoom(),
    pitch: mapInstance.getPitch(),
    bearing: mapInstance.getBearing()
  };

  // Hide route line and waypoints
  if (mapInstance.getLayer('route-line')) {
    mapInstance.setLayoutProperty('route-line', 'visibility', 'none');
  }
  if (mapInstance.getLayer('route-line-outline')) {
    mapInstance.setLayoutProperty('route-line-outline', 'visibility', 'none');
  }
  waypointMarkers.forEach(m => m.getElement().style.display = 'none');

  // Stop any existing animation
  stopAnimation();

  // Start game timer only — Three.js handles all rendering and clicks
  gameTimerInterval = setInterval(() => {
    window.dispatchEvent(new CustomEvent('game-tick', { detail: { elapsed: 100 } }));
  }, 100);
}

let gameClickEnabled = false;
let gameMoving = false;

function gameClickHandler(e) {
  if (!gameMode || gameMoving || !gameClickEnabled) return;
  console.log('[game] Click at', e.lngLat.lat.toFixed(5), e.lngLat.lng.toFixed(5));
  window.dispatchEvent(new CustomEvent('game-click', {
    detail: { lat: e.lngLat.lat, lon: e.lngLat.lng }
  }));
}

let savedCameraMode = null;
let gamePaused = false;
let gameSpeedMultiplier = 1.0;
let gamePausedAt = null; // timestamp when paused
let gamePausedTotal = 0; // total ms spent paused

function gameAnimateCamera(timestamp) {
  if (!currentRoute || currentRoute.length < 2 || !gameMode) {
    stopAnimation();
    return;
  }

  if (animationStartTimestamp === null) {
    animationStartTimestamp = timestamp;
  }

  const mode = currentCameraMode;
  const duration = animationDurationMs;
  const elapsed = timestamp - animationStartTimestamp - gamePausedTotal;
  const progress = Math.min(elapsed / duration, 1.0);
  const targetDistance = routeLengthMeters * progress;
  const cameraPoint = coordinateAtDistance(targetDistance);
  const lookAheadDistance = Math.min(targetDistance + mode.lookahead, routeLengthMeters);
  const lookAtPoint = coordinateAtDistance(lookAheadDistance);

  const pathBearing = calculateBearing(
    [cameraPoint.lon, cameraPoint.lat],
    [lookAtPoint.lon, lookAtPoint.lat]
  );

  const smoothedBearing = smoothAngle(lastBearing, pathBearing, mode.smoothing);
  lastBearing = smoothedBearing;

  mapInstance.jumpTo({
    center: [cameraPoint.lon, cameraPoint.lat],
    bearing: smoothedBearing,
    pitch: mode.pitch,
    zoom: mode.zoom
  });

  // Update player marker position and rotation
  if (playerMarker) {
    playerMarker.setLngLat([cameraPoint.lon, cameraPoint.lat]);
    playerMarker.setRotation(smoothedBearing);
  }

  // Send position to Elm for control point detection
  window.dispatchEvent(new CustomEvent('player-position', {
    detail: { lat: cameraPoint.lat, lon: cameraPoint.lon }
  }));

  // Send bearing for compass
  window.dispatchEvent(new CustomEvent('game-bearing', {
    detail: { bearing: smoothedBearing }
  }));

  if (progress < 1.0) {
    animationFrameId = requestAnimationFrame(gameAnimateCamera);
  } else {
    // Route finished
    animationFrameId = null;
    window.dispatchEvent(new CustomEvent('player-movement-done'));
  }
}

export function exitFirstPersonMode() {
  if (!mapInstance) return;
  console.log('[game] Exiting first-person mode');

  gameMode = false;

  // Remove game-mode class from body
  document.body.classList.remove('game-mode');

  // Restore z-index chain
  let el = document.querySelector('.app-container');
  while (el && el !== document.body) {
    el.style.zIndex = '';
    el.style.position = '';
    el = el.parentElement;
  }

  // Stop movement animation
  if (moveAnimationId) {
    cancelAnimationFrame(moveAnimationId);
    moveAnimationId = null;
  }

  // Stop timer
  if (gameTimerInterval) {
    clearInterval(gameTimerInterval);
    gameTimerInterval = null;
  }

  // Remove player marker
  if (playerMarker) {
    playerMarker.remove();
    playerMarker = null;
  }

  // Remove control point markers
  controlPointMarkers.forEach(m => m.remove());
  controlPointMarkers = [];

  // Remove game click handler
  mapInstance.off('click', gameClickHandler);
  gameMoving = false;

  // Restore camera mode
  if (savedCameraMode) {
    currentCameraMode = savedCameraMode;
    savedCameraMode = null;
  }

  // Re-enable map interactions
  mapInstance.dragPan.enable();
  mapInstance.scrollZoom.enable();
  mapInstance.doubleClickZoom.enable();

  // Restore topo style and resize back
  switchMapStyle('topo');
  setTimeout(() => mapInstance.resize(), 100);

  // Restore route and waypoint visibility
  if (mapInstance.getLayer('route-line')) {
    mapInstance.setLayoutProperty('route-line', 'visibility', 'visible');
  }
  if (mapInstance.getLayer('route-line-outline')) {
    mapInstance.setLayoutProperty('route-line-outline', 'visibility', 'visible');
  }
  waypointMarkers.forEach(m => m.getElement().style.display = '');

  // Restore camera
  if (savedCameraState) {
    mapInstance.flyTo({ ...savedCameraState, duration: 1000 });
    savedCameraState = null;
  }
}


export function movePlayerAlongPath(coords) {
  if (!mapInstance || coords.length < 2) return;
  console.log('[game] Walking to destination,', coords.length, 'points');

  gameMoving = true;

  // Calculate segment distances
  const segments = [];
  let totalDist = 0;
  for (let i = 1; i < coords.length; i++) {
    const dlat = (coords[i].lat - coords[i-1].lat) * Math.PI / 180;
    const dlon = (coords[i].lon - coords[i-1].lon) * Math.PI / 180;
    const lat1 = coords[i-1].lat * Math.PI / 180;
    const lat2 = coords[i].lat * Math.PI / 180;
    const a = Math.sin(dlat/2)**2 + Math.cos(lat1)*Math.cos(lat2)*Math.sin(dlon/2)**2;
    const d = 2 * 6371000 * Math.asin(Math.sqrt(a));
    segments.push(d);
    totalDist += d;
  }

  const speed = 3.0 * gameSpeedMultiplier; // m/s
  const durationMs = (totalDist / speed) * 1000;
  const startTime = performance.now();

  function animate(now) {
    if (gamePaused) {
      moveAnimationId = requestAnimationFrame(animate);
      return;
    }

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

    const segProgress = segments[segIdx] > 0
      ? (targetDist - accumulated) / segments[segIdx] : 0;

    const from = coords[segIdx];
    const to = coords[Math.min(segIdx + 1, coords.length - 1)];
    const lat = from.lat + (to.lat - from.lat) * segProgress;
    const lon = from.lon + (to.lon - from.lon) * segProgress;

    // Bearing towards next point
    const dLon = (to.lon - from.lon) * Math.PI / 180;
    const y = Math.sin(dLon) * Math.cos(to.lat * Math.PI / 180);
    const x = Math.cos(from.lat * Math.PI / 180) * Math.sin(to.lat * Math.PI / 180)
            - Math.sin(from.lat * Math.PI / 180) * Math.cos(to.lat * Math.PI / 180) * Math.cos(dLon);
    const bearing = ((Math.atan2(y, x) * 180 / Math.PI) + 360) % 360;

    // Update player marker
    if (playerMarker) {
      playerMarker.setLngLat([lon, lat]);
      playerMarker.setRotation(bearing);
    }

    // Camera follows player — immersive 3D view
    mapInstance.jumpTo({
      center: [lon, lat],
      bearing: bearing,
      pitch: 65,
      zoom: 17.5
    });

    // Send position to Elm
    window.dispatchEvent(new CustomEvent('player-position', {
      detail: { lat, lon }
    }));
    window.dispatchEvent(new CustomEvent('game-bearing', {
      detail: { bearing }
    }));

    if (progress < 1.0) {
      moveAnimationId = requestAnimationFrame(animate);
    } else {
      // Send final exact position (last point of route)
      const finalPos = coords[coords.length - 1];
      if (playerMarker) playerMarker.setLngLat([finalPos.lon, finalPos.lat]);
      window.dispatchEvent(new CustomEvent('player-position', {
        detail: { lat: finalPos.lat, lon: finalPos.lon }
      }));

      moveAnimationId = null;
      gameMoving = false;
      window.dispatchEvent(new CustomEvent('player-movement-done'));
    }
  }

  if (moveAnimationId) cancelAnimationFrame(moveAnimationId);
  moveAnimationId = requestAnimationFrame(animate);
}

export function pauseGame() {
  gamePaused = true;
  gamePausedAt = performance.now();
  if (animationFrameId !== null) {
    cancelAnimationFrame(animationFrameId);
    animationFrameId = null;
  }
  console.log('[game] Paused');
}

export function resumeGame() {
  if (gamePausedAt !== null) {
    gamePausedTotal += performance.now() - gamePausedAt;
    gamePausedAt = null;
  }
  gamePaused = false;
  animationFrameId = requestAnimationFrame(gameAnimateCamera);
  console.log('[game] Resumed');
}

export function setGameSpeedMultiplier(speed) {
  gameSpeedMultiplier = speed;
  // Recalculate animation duration
  if (routeLengthMeters > 0) {
    animationDurationMs = (routeLengthMeters / (CAMERA_MODES.WALKING.speed * gameSpeedMultiplier)) * 1000;
  }
  console.log('[game] Speed:', speed, 'x, duration:', (animationDurationMs / 1000).toFixed(0), 's');
}

// Show/hide topo map overlay (overhead view with balises, no player position)
export function showTopoOverlayMode(show) {
  if (!mapInstance) return;
  console.log('[game] Topo overlay:', show);

  if (show) {
    // Switch to topo IGN style, overhead view with balises, no player position
    switchMapStyle('topo');
    mapInstance.setTerrain(null);
    mapInstance.jumpTo({
      pitch: 0,
      zoom: 14,
      bearing: 0
    });
    // Hide player marker (no GPS in CO!)
    if (playerMarker) playerMarker.getElement().style.display = 'none';
  } else {
    // Back to hybrid 3D walking view (satellite + roads visible)
    switchMapStyle('hybrid');
    mapInstance.setTerrain({ source: 'terrainSource', exaggeration: 1.5 });
    terrainEnabled = true;
    // Show player marker
    if (playerMarker) playerMarker.getElement().style.display = '';
  }
}

// Reveal a single control point marker (when player is within 10m)
let revealedCpMarker = null;
export function revealNearbyCP(data) {
  if (!mapInstance) return;
  console.log('[game] Revealing control point:', data.label);

  // Remove previous revealed marker
  if (revealedCpMarker) {
    revealedCpMarker.remove();
    revealedCpMarker = null;
  }

  const el = document.createElement('div');
  el.className = 'game-control-point found';
  el.innerHTML = `<div class="cp-circle">${data.label}</div>`;
  revealedCpMarker = new maplibregl.Marker({ element: el })
    .setLngLat([data.lon, data.lat])
    .addTo(mapInstance);
}

// Hide all control point markers (back to 3D walking view)
export function hideAllCPs() {
  controlPointMarkers.forEach(m => m.remove());
  controlPointMarkers = [];
  if (revealedCpMarker) {
    revealedCpMarker.remove();
    revealedCpMarker = null;
  }
}

export function updateGameControlPoints(points) {
  if (!mapInstance) return;

  // Remove existing markers
  controlPointMarkers.forEach(m => m.remove());
  controlPointMarkers = [];

  points.forEach((cp) => {
    const el = document.createElement('div');
    el.className = `game-control-point ${cp.found ? 'found' : ''}`;
    el.innerHTML = `<div class="cp-circle">${cp.label}</div>`;

    const marker = new maplibregl.Marker({ element: el })
      .setLngLat([cp.lon, cp.lat])
      .addTo(mapInstance);
    controlPointMarkers.push(marker);
  });
}

