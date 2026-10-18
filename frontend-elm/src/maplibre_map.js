import maplibregl from 'maplibre-gl';
import mlcontour from 'maplibre-contour';

import {
  createFlyoverControl, initFlyover, setFlyoverElevations, setFlyoverRoute,
  setFlyoverStages, setFlyoverWaypoints, startFlyover, stopFlyover
} from './flyover.js';
import { createGrip, makeDraggable } from './draggable.js';
import { casteIgn, largeurMiseALEchelle, symboDuFiltre } from './chemin_noir.js';

/**
 * Couleur de carte lue dans le design system.
 *
 * Les teintes de la carte étaient écrites ici, en dur : vingt-deux valeurs
 * dont les défauts de Material Design, à côté d'un habillage en laiton et
 * vert forêt. Elles vivent désormais dans `style.css` avec le reste de la
 * palette, et ce lecteur est le seul chemin pour y accéder.
 *
 * Pas de valeur de repli : un repli serait une seconde définition, donc
 * exactement le problème qu'on vient de corriger. Si la feuille de style
 * n'est pas chargée, le magenta et l'avertissement rendent la panne visible
 * plutôt que de laisser croire à un simple choix de couleur.
 */
function mapColor(token) {
  const value = getComputedStyle(document.documentElement)
    .getPropertyValue(token)
    .trim();

  if (!value) {
    console.warn(`[maplibre] jeton ${token} introuvable : feuille de style non chargée ?`);
    return '#ff00ff';
  }

  return value;
}

let mapInstance;
let routeSource;
let startMarker;
let elevationMarker = null; // Point du tracé suivi par la bande de dénivelé
let endMarker;
let clickHandlerSet = false;
let terrainEnabled = false;
let bboxLayer;
let currentRoute = null;
let waypointMarkers = []; // Markers for multi-point route waypoints
let currentMapStyle = 'topo'; // 'topo' | 'satellite' | 'hybrid'
let kmMarkers = []; // Kilometer markers along route
let poiMarkers = []; // POI markers on map
let poisVisible = false; // POI toggle state
let lastPoiBbox = null; // Last fetched POI bbox to avoid re-fetching
let waterMarkers = []; // Water point markers along current route
let waterEnabled = false; // Opt-in: 103 markers on a 450km route drown the map
let currentRouteCoords = null; // Kept so toggling on can fetch without a new route

// Terrain configuration - Using Terrarium format tiles from AWS
const TERRAIN_EXAGGERATION = 2.5; // Amplify terrain for better visibility
const EARTH_RADIUS_M = 6371000;
const DRONE_PITCH = 60; // Default pitch for 3D drone view

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
      // Font server for symbol layers. Without it, every text-field is silently
      // dropped — which is why the contour elevation labels never appeared.
      glyphs: IGN_GLYPHS,
      sources: {
        'osm': {
          type: 'raster',
          tiles: ['https://tile.openstreetmap.org/{z}/{x}/{y}.png'],
          // Left at 256 on purpose: the OSM tile usage policy rules out the 4x
          // traffic the HiDPI trick costs.
          tileSize: 256,
          maxzoom: 19,
          attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors'
        },
        'satellite': {
          type: 'raster',
          tiles: [
            'https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}'
          ],
          tileSize: 128,
          maxzoom: 19,
          attribution: '© Esri'
        },
        // Géoplateforme IGN — free, no API key. SCAN 25 / SCAN Express are NOT
        // here: they are licensed products, absent from the open WMTS.
        'ign-plan': {
          type: 'raster',
          tiles: [ignWmtsUrl('GEOGRAPHICALGRIDSYSTEMS.PLANIGNV2', 'image/png')],
          // 128 makes MapLibre fetch one zoom level deeper for the same screen
          // area: twice the pixel density, up to the layer's native ceiling.
          tileSize: 128,
          maxzoom: 19,
          attribution: IGN_ATTRIBUTION
        },
        'ign-ortho': {
          type: 'raster',
          // Orthophotos are served as JPEG; asking for PNG returns a 400.
          tiles: [ignWmtsUrl('ORTHOIMAGERY.ORTHOPHOTOS', 'image/jpeg')],
          tileSize: 128,
          maxzoom: 19,
          attribution: IGN_ATTRIBUTION
        },
        'ign-slopes': {
          type: 'raster',
          tiles: [ignWmtsUrl('GEOGRAPHICALGRIDSYSTEMS.SLOPES.MOUNTAIN', 'image/png')],
          tileSize: 256,
          maxzoom: 17,
          attribution: IGN_ATTRIBUTION
        },
        // Public forests (domaniales, communales…). Only served with the ONF
        // style on the PM_3_16 grid: zooms 3 to 16, overzoomed beyond.
        'ign-forests': {
          type: 'raster',
          tiles: [ignWmtsUrl('FORETS.PUBLIQUES', 'image/png', 'FORETS PUBLIQUES ONF', 'PM_3_16')],
          tileSize: 256,
          minzoom: 3,
          maxzoom: 16,
          attribution: IGN_ATTRIBUTION
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

  // Poignée de debug, posée dès la construction et non sur `load` : la caméra
  // (`fitBounds`, `cameraForBounds`, `project`) répond avant le premier rendu,
  // et `load` ne tire pas du tout quand le contexte WebGL ne peint pas.
  window.__mapInstance = mapInstance;

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
      paint: { 'hillshade-shadow-color': mapColor('--map-hillshade') }
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
        // IGN's font server has Open Sans, not Noto Sans (404).
        'text-font': ['Open Sans Regular'],
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
    // Calques de référence : des itinéraires enregistrés affichés en
    // surimpression, chacun de sa couleur, pour comparer le tracé courant.
    // Déclarés avant la route pour rester dessous.
    mapInstance.addSource('reference', {
      type: 'geojson',
      data: { type: 'FeatureCollection', features: [] }
    });

    // Fin et plein, cerné de blanc : il se lit sur tous les fonds sans
    // masquer la carte ni le tracé courant, qui passe par-dessus.
    mapInstance.addLayer({
      id: 'reference-line-halo',
      type: 'line',
      source: 'reference',
      layout: { 'line-join': 'round', 'line-cap': 'round' },
      paint: {
        'line-color': '#ffffff',
        'line-width': ['interpolate', ['linear'], ['zoom'], 8, 7, 16, 9],
        'line-opacity': 0.9
      }
    });

    mapInstance.addLayer({
      id: 'reference-line',
      type: 'line',
      source: 'reference',
      layout: { 'line-join': 'round', 'line-cap': 'round' },
      paint: {
        'line-color': ['get', 'color'],
        'line-width': ['interpolate', ['linear'], ['zoom'], 8, 3, 16, 5],
        'line-opacity': 0.9
      }
    });

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
        'line-color': mapColor('--map-route-halo'),
        'line-width': 6,
        'line-opacity': 0.8
      }
    });

    // Add main route layer on top (paved = green)
    mapInstance.addLayer({
      id: 'route-line',
      type: 'line',
      source: 'route',
      layout: {
        'line-join': 'round',
        'line-cap': 'round'
      },
      paint: {
        'line-color': mapColor('--map-route-paved'),
        'line-width': 4
      }
    });

    // Unpaved OSM background overlay (all tracks/paths on map)
    mapInstance.addSource('osm-unpaved', {
      type: 'geojson',
      data: { type: 'FeatureCollection', features: [] }
    });
    mapInstance.addLayer({
      id: 'osm-unpaved-line',
      type: 'line',
      source: 'osm-unpaved',
      layout: { 'line-join': 'round', 'line-cap': 'round', visibility: 'none' },
      paint: { 'line-color': mapColor('--map-osm-unpaved'), 'line-width': 2, 'line-opacity': 0.7, 'line-dasharray': [3, 2] }
    });

    // Lodging overlay (OSM + refuges.info), filled on demand by the 🛏️ control
    mapInstance.addSource('lodging', {
      type: 'geojson',
      data: { type: 'FeatureCollection', features: [] }
    });
    mapInstance.addLayer({
      id: 'lodging-points',
      type: 'circle',
      source: 'lodging',
      layout: { visibility: 'none' },
      paint: {
        'circle-radius': 6,
        'circle-color': ['match', ['get', 'kind'],
          'free', mapColor('--map-lodging-free'),
          'camp', mapColor('--map-lodging-camp'),
          mapColor('--map-lodging-paid')],
        'circle-stroke-color': mapColor('--map-marker-face'),
        'circle-stroke-width': 1.5
      }
    });

    // Unpaved route overlay (purple), drawn above paved layer
    mapInstance.addSource('route-unpaved', {
      type: 'geojson',
      data: { type: 'FeatureCollection', features: [] }
    });
    mapInstance.addLayer({
      id: 'route-line-unpaved-outline',
      type: 'line',
      source: 'route-unpaved',
      layout: { 'line-join': 'round', 'line-cap': 'round' },
      paint: { 'line-color': mapColor('--map-route-halo'), 'line-width': 6, 'line-opacity': 0.6 }
    });
    mapInstance.addLayer({
      id: 'route-line-unpaved',
      type: 'line',
      source: 'route-unpaved',
      layout: { 'line-join': 'round', 'line-cap': 'round' },
      paint: { 'line-color': mapColor('--map-route-unpaved'), 'line-width': 4 }
    });

    // Passages par un chemin que seule l'IGN connaît : OSM ne l'a pas, il a
    // pu disparaître sur le terrain. Une teinte à part pour le vérifier.
    mapInstance.addSource('route-ign', {
      type: 'geojson',
      data: { type: 'FeatureCollection', features: [] }
    });
    mapInstance.addLayer({
      id: 'route-line-ign',
      type: 'line',
      source: 'route-ign',
      layout: { 'line-join': 'round', 'line-cap': 'round' },
      paint: { 'line-color': mapColor('--map-route-ign'), 'line-width': 4 }
    });

    // Tronçons tirés tout droit, en pointillé par-dessus le tracé, et ce
    // qu'ils traversent.
    mapInstance.addSource('route-direct', {
      type: 'geojson',
      data: { type: 'FeatureCollection', features: [] }
    });
    mapInstance.addLayer({
      id: 'route-line-direct',
      type: 'line',
      source: 'route-direct',
      layout: { 'line-join': 'round', 'line-cap': 'butt' },
      paint: { 'line-color': mapColor('--map-route-direct'), 'line-width': 3, 'line-dasharray': [2, 2] }
    });
    mapInstance.addSource('route-obstacles', {
      type: 'geojson',
      data: { type: 'FeatureCollection', features: [] }
    });
    mapInstance.addLayer({
      id: 'route-obstacles-dots',
      type: 'circle',
      source: 'route-obstacles',
      paint: {
        'circle-radius': 5,
        'circle-stroke-width': 1.5,
        'circle-stroke-color': mapColor('--map-route-halo'),
        'circle-color': [
          'match', ['get', 'kind'],
          'building', mapColor('--map-obstacle-building'),
          'open_ground', mapColor('--map-obstacle-open'),
          'paved_road', mapColor('--map-obstacle-road'),
          'steep_slope', mapColor('--map-obstacle-steep'),
          mapColor('--map-route-direct')
        ]
      }
    });
    _applyDirectOverlay();
    _applyReferenceRoutes();

    console.debug('[maplibre] Map loaded with terrain support');
  });

  // Add navigation controls
  mapInstance.addControl(new maplibregl.NavigationControl({
    visualizePitch: true
  }), 'top-right');

  // Add terrain toggle control
  const terrainControl = createTerrainControl();
  mapInstance.addControl(terrainControl, 'top-right');

  // Survol 3D : bouton ▶, le reste vit dans son panneau.
  initFlyover(mapInstance, { enableTerrain: enableTerrainForFlyover, mapColor });
  mapInstance.addControl(createFlyoverControl(), 'top-right');

  // Add pitch toggle control (for free tilt)
  const pitchControl = createPitchControl();
  mapInstance.addControl(pitchControl, 'top-right');

  // Add unpaved paths overlay toggle
  mapInstance.addControl(createUnpavedControl(), 'top-right');

  // Add POI toggle control
  const poiControl = createPoiControl();
  mapInstance.addControl(poiControl, 'top-right');

  // Add IGN slope map toggle control (off by default)
  mapInstance.addControl(createSlopesControl(), 'top-right');

  // Add public forests toggle control (off by default)
  mapInstance.addControl(createForestsControl(), 'top-right');

  // Add lodging toggle control (off by default)
  mapInstance.addControl(createLodgingControl(), 'top-right');

  // Add stage planner control: nights in lodgings along the route
  mapInstance.addControl(createStagesControl(), 'top-right');

  // Add water points toggle control (off by default)
  mapInstance.addControl(createWaterControl(), 'top-right');

  // Add fullscreen toggle control
  mapInstance.addControl(createFullscreenControl(), 'top-right');

  // Add map style switcher (topo / satellite / hybrid)
  const styleControl = createStyleSwitcherControl();
  mapInstance.addControl(styleControl, 'bottom-left');
}

const IGN_GLYPHS =
  'https://data.geopf.fr/annexes/ressources/vectorTiles/fonts/{fontstack}/{range}.pbf';

const IGN_VECTOR_STYLE_URL =
  'https://data.geopf.fr/annexes/ressources/vectorTiles/styles/PLAN.IGN/standard.json';

const IGN_ATTRIBUTION =
  '&copy; <a href="https://geoservices.ign.fr/">IGN-F/G\u00e9oplateforme</a>';

/**
 * WMTS tile URL template for a Géoplateforme layer.
 *
 * Most layers use the `normal` style on the full `PM` grid; a few only answer
 * with their own style and a reduced grid (see the WMTS GetCapabilities).
 */
function ignWmtsUrl(layer, format, style = 'normal', matrixSet = 'PM') {
  return (
    'https://data.geopf.fr/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0' +
    '&LAYER=' + layer +
    '&STYLE=' + encodeURIComponent(style) +
    '&TILEMATRIXSET=' + matrixSet +
    '&TILEMATRIX={z}&TILEROW={y}&TILECOL={x}' +
    '&FORMAT=' + encodeURIComponent(format)
  );
}

/**
 * Button toggling an IGN raster overlay, off by default.
 *
 * The layer is only added on first use, so a closed overlay costs no tiles.
 */
function createRasterOverlayControl({ source, layerId, icon, title, className, colorToken, opacity }) {
  class RasterOverlayControl {
    onAdd(map) {
      this._map = map;
      this._enabled = false;
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';
      this._btn = document.createElement('button');
      this._btn.className = className;
      this._btn.textContent = icon;
      this._btn.title = title;
      this._btn.onclick = () => this.toggle();
      this._container.appendChild(this._btn);
      return this._container;
    }

    toggle() {
      this._enabled = !this._enabled;

      // Sits above the background but below the route, which ensureRouteLayers
      // keeps on top anyway.
      if (this._enabled && !this._map.getLayer(layerId)) {
        this._map.addLayer({
          id: layerId,
          type: 'raster',
          source,
          minzoom: 0,
          maxzoom: 22,
          paint: { 'raster-opacity': opacity }
        });
        ensureRouteLayers();
      }

      setLayerVisible(layerId, this._enabled);
      this._btn.style.backgroundColor = this._enabled ? mapColor(colorToken) : '';
      this._btn.style.color = this._enabled ? 'white' : '';
    }

    onRemove() {
      this._container.parentNode.removeChild(this._container);
      this._map = undefined;
    }
  }
  return new RasterOverlayControl();
}

function createSlopesControl() {
  return createRasterOverlayControl({
    source: 'ign-slopes',
    layerId: 'ign-slopes-tiles',
    icon: '📐',
    title: 'Afficher/masquer la carte des pentes IGN (>30°)',
    className: 'maplibregl-ctrl-slopes',
    colorToken: '--map-toggle-slopes',
    opacity: 0.55
  });
}

/** Public forests: where a route can cross freely, unlike private woods. */
function createForestsControl() {
  return createRasterOverlayControl({
    source: 'ign-forests',
    layerId: 'ign-forests-tiles',
    icon: '🌲',
    title: 'Afficher/masquer les forêts publiques (IGN/ONF)',
    className: 'maplibregl-ctrl-forests',
    colorToken: '--map-toggle-forests',
    opacity: 0.45
  });
}

function createStyleSwitcherControl() {
  const STYLES = [
    { id: 'topo',      label: 'Topo',      icon: '◈' },
    { id: 'ign-plan',  label: 'Plan IGN',  icon: '◆' },
    { id: 'chemin-noir', label: 'Chemin noir', icon: '◐' },
    { id: 'ign-ortho', label: 'Ortho IGN', icon: '◈' },
    { id: 'satellite', label: 'Satellite', icon: '◉' },
    { id: 'hybrid',    label: 'Hybride',   icon: '◎' }
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

      const grip = createGrip();
      this._container.prepend(grip);
      makeDraggable(this._container, grip, 'cn.drag.style-switcher');
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
      if (currentPitch < 10) {
        // Enable terrain + tilt to 55° for visible relief
        if (!terrainEnabled) {
          terrainEnabled = true;
          this._map.setTerrain({ source: 'terrainSource', exaggeration: TERRAIN_EXAGGERATION });
        }
        this._map.easeTo({ pitch: 55, duration: 800 });
        this._button.textContent = '0°';
      } else {
        this._map.setTerrain(null);
        terrainEnabled = false;
        this._map.easeTo({ pitch: 0, duration: 800 });
        this._button.textContent = '45°';
      }
    }

    onRemove() {
      this._container.parentNode.removeChild(this._container);
      this._map = undefined;
    }
  }
  return new PitchControl();
}

// ── Lodging overlay (OSM + refuges.info) ──────────────────────────────────
let lodgingEnabled = false;
let lodgingFetchTimer = null;
let lodgingFetching = false;
let lodgingRefetch = false; // the map moved while a fetch was running
let lodgingPopup = null;
/** Every lodging seen, by url: a failed or slow request no longer wipes the map. */
const lodgingCache = new Map();

/** OSM tourism=* → [libellé, catégorie]. paid / free / camp pilotent la couleur. */
const OSM_LODGING = {
  hotel: ['hôtel', 'paid'],
  motel: ['motel', 'paid'],
  guest_house: ["chambre d'hôtes", 'paid'],
  hostel: ['auberge de jeunesse', 'paid'],
  chalet: ['gîte', 'paid'],
  apartment: ['location', 'paid'],
  alpine_hut: ['refuge gardé', 'free'],
  wilderness_hut: ['cabane', 'free'],
  camp_site: ['camping', 'camp']
};

/** overpass-api.de sature souvent (timeout, 502 sans CORS) : on passe au miroir suivant. */
const OVERPASS_SERVERS = [
  'https://overpass-api.de/api/interpreter',
  'https://overpass.kumi.systems/api/interpreter',
  'https://overpass.private.coffee/api/interpreter'
];

async function overpassQuery(query) {
  const errors = [];
  for (const url of OVERPASS_SERVERS) {
    try {
      const res = await fetch(url, {
        method: 'POST',
        body: 'data=' + encodeURIComponent(query),
        signal: AbortSignal.timeout(12000)
      });
      if (!res.ok) throw new Error(String(res.status));
      return await res.json();
    } catch (e) {
      errors.push(`${new URL(url).host}: ${e.message}`);
    }
  }
  throw new Error(errors.join(' ; '));
}

async function fetchOsmLodging(b) {
  const bbox = `${b.getSouth().toFixed(4)},${b.getWest().toFixed(4)},${b.getNorth().toFixed(4)},${b.getEast().toFixed(4)}`;
  const types = Object.keys(OSM_LODGING).join('|');
  const query = `[out:json][timeout:10];nwr["tourism"~"^(${types})$"](${bbox});out center tags qt;`;
  const data = await overpassQuery(query);
  return (data.elements || []).flatMap(el => {
    const lat = el.lat ?? el.center?.lat;
    const lon = el.lon ?? el.center?.lon;
    const [label, kind] = OSM_LODGING[el.tags?.tourism] || [];
    if (lat == null || !label) return [];
    return [{
      lon, lat, kind, label,
      name: el.tags.name || label,
      source: 'OSM',
      url: `https://www.openstreetmap.org/${el.type}/${el.id}`
    }];
  });
}

async function fetchRefugesInfo(b) {
  const bbox = [b.getWest(), b.getSouth(), b.getEast(), b.getNorth()].map(v => v.toFixed(4)).join(',');
  const url = 'https://www.refuges.info/api/bbox?' + new URLSearchParams({
    bbox, type_points: 'cabane,refuge,gite', format: 'geojson', detail: 'simple'
  });
  const res = await fetch(url, { signal: AbortSignal.timeout(15000) });
  if (!res.ok) throw new Error('refuges.info ' + res.status);
  const data = await res.json();
  return (data.features || []).map(f => {
    const label = f.properties.type?.valeur || 'refuge';
    return {
      lon: f.geometry.coordinates[0],
      lat: f.geometry.coordinates[1],
      kind: label.startsWith('gîte') ? 'paid' : 'free',
      label,
      name: f.properties.nom || label,
      source: 'refuges.info',
      url: `https://www.refuges.info/point/${f.id}`
    };
  });
}

/** Les deux bases recensent souvent le même refuge : on garde la fiche refuges.info. */
function dedupeLodging(refuges, osm) {
  const near = (a, b) => haversineMeters(a, b) < 100 && a.kind === b.kind;
  return [...refuges, ...osm.filter(o => !refuges.some(r => near(r, o)))];
}

function setLodgingData(items) {
  const src = mapInstance?.getSource('lodging');
  if (!src) return;
  src.setData({
    type: 'FeatureCollection',
    features: items.map(({ lon, lat, ...props }) => ({
      type: 'Feature',
      geometry: { type: 'Point', coordinates: [lon, lat] },
      properties: props
    }))
  });
}

async function fetchLodging() {
  if (!mapInstance || !lodgingEnabled) return;
  // Zoomed out: keep what is already known but ask for nothing new.
  if (mapInstance.getZoom() < 11) return;
  if (lodgingFetching) {
    lodgingRefetch = true;
    return;
  }
  lodgingFetching = true;

  const b = mapInstance.getBounds();
  try {
    const [osm, refuges] = await Promise.all([
      fetchOsmLodging(b).catch(e => { console.warn('[lodging] OSM:', e.message); return []; }),
      fetchRefugesInfo(b).catch(e => { console.warn('[lodging] refuges.info:', e.message); return []; })
    ]);
    const cached = [...lodgingCache.values()];
    const fresh = dedupeLodging(refuges, osm)
      .filter(l => !cached.some(c => c.url !== l.url && haversineMeters(c, l) < 100 && c.kind === l.kind));
    fresh.forEach(l => lodgingCache.set(l.url, l));
    if (lodgingEnabled) setLodgingData([...lodgingCache.values()]);
    console.log('[lodging] loaded', fresh.length, 'hébergements,', lodgingCache.size, 'en tout');
  } finally {
    lodgingFetching = false;
  }
  if (lodgingRefetch) {
    lodgingRefetch = false;
    fetchLodging();
  }
}

function scheduleLodgingFetch() {
  if (!lodgingEnabled) return;
  clearTimeout(lodgingFetchTimer);
  lodgingFetchTimer = setTimeout(fetchLodging, 800);
}

function showLodgingPopup(event) {
  const f = event.features?.[0];
  if (!f) return;
  const p = f.properties;
  const box = document.createElement('div');
  box.className = 'place-popup';
  box.appendChild(placeRow(p.name, `${p.label} · ${p.source}`, p.name));
  const link = document.createElement('a');
  link.href = p.url;
  link.target = '_blank';
  link.rel = 'noopener noreferrer';
  link.textContent = 'Voir la fiche';
  box.appendChild(link);
  lodgingPopup?.remove();
  lodgingPopup = new maplibregl.Popup({ offset: 8, maxWidth: '280px' })
    .setLngLat(f.geometry.coordinates)
    .setDOMContent(box)
    .addTo(mapInstance);
}

// Survol d'un hébergement : nom et type dans une infobulle légère.
let lodgingHover = null;

const setPointer = (event) => {
  mapInstance.getCanvas().style.cursor = 'pointer';
  const f = event.features?.[0];
  if (!f) return;
  const p = f.properties;
  lodgingHover?.remove();
  const box = document.createElement('div');
  const name = document.createElement('strong');
  name.textContent = p.name;
  const kind = document.createElement('span');
  kind.className = `lodging-hover-kind lodging-kind-${p.kind}`;
  kind.textContent = p.label;
  box.append(name, kind);
  lodgingHover = new maplibregl.Popup({
    offset: 10,
    closeButton: false,
    closeOnClick: false,
    className: 'lodging-hover'
  })
    .setLngLat(f.geometry.coordinates)
    .setDOMContent(box)
    .addTo(mapInstance);
};
const unsetPointer = () => {
  mapInstance.getCanvas().style.cursor = '';
  lodgingHover?.remove();
  lodgingHover = null;
};

function createLodgingControl() {
  class LodgingControl {
    onAdd(map) {
      this._map = map;
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';
      this._btn = document.createElement('button');
      this._btn.className = 'maplibregl-ctrl-lodging';
      this._btn.textContent = '🛏️';
      this._btn.title = 'Afficher/masquer les hébergements (OSM, refuges.info) — zoom 11+';
      this._btn.onclick = () => this.toggle();
      this._container.appendChild(this._btn);
      return this._container;
    }

    toggle() {
      lodgingEnabled = !lodgingEnabled;
      setLayerVisible('lodging-points', lodgingEnabled);
      this._btn.style.backgroundColor = lodgingEnabled ? mapColor('--map-toggle-lodging') : '';
      this._btn.style.color = lodgingEnabled ? 'white' : '';

      if (lodgingEnabled) {
        setLodgingData([...lodgingCache.values()]);
        fetchLodging();
        this._map.on('moveend', scheduleLodgingFetch);
        this._map.on('click', 'lodging-points', showLodgingPopup);
        this._map.on('mouseenter', 'lodging-points', setPointer);
        this._map.on('mouseleave', 'lodging-points', unsetPointer);
      } else {
        this._map.off('moveend', scheduleLodgingFetch);
        this._map.off('click', 'lodging-points', showLodgingPopup);
        this._map.off('mouseenter', 'lodging-points', setPointer);
        this._map.off('mouseleave', 'lodging-points', unsetPointer);
        clearTimeout(lodgingFetchTimer);
        lodgingPopup?.remove();
        unsetPointer();
        setLodgingData([]);
      }
    }

    onRemove() {
      this._container.parentNode.removeChild(this._container);
      this._map = undefined;
    }
  }
  return new LodgingControl();
}

// ── Stage planner: one night per lodging, ~25 km a day ────────────────────
const STAGE_TARGET_KM = 25;
const STAGE_TOLERANCE_KM = 5;
const STAGE_DETOUR_M = 5000;
/** Refuges exclus : on veut un lit ou un emplacement de camping. */
const STAGE_KINDS = ['hotel', 'motel', 'guest_house', 'hostel', 'chalet', 'apartment', 'camp_site'];
const STAGE_KIND_LABELS = {
  hotel: 'hôtel', motel: 'motel', guest_house: "chambre d'hôtes", hostel: 'auberge',
  chalet: 'gîte', apartment: 'location', camp_site: 'camping',
  alpine_hut: 'refuge', wilderness_hut: 'cabane'
};

let stageMarkers = [];
let stagesPanel = null;

function clearStages() {
  setFlyoverStages(null);
  stageMarkers.forEach(m => m.remove());
  stageMarkers = [];
  stagesPanel?.remove();
  stagesPanel = null;
}

async function postJson(url, body) {
  const res = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body)
  });
  if (!res.ok) throw new Error(`${url} ${res.status} ${await res.text()}`);
  return res.json();
}

function stagesPanelShell(title) {
  stagesPanel?.remove();
  const panel = document.createElement('div');
  panel.className = 'stages-panel';
  const head = document.createElement('div');
  head.className = 'stages-head';
  const h = document.createElement('strong');
  h.textContent = title;
  const close = document.createElement('button');
  close.className = 'stages-close';
  close.textContent = '✕';
  close.title = 'Fermer les étapes';
  close.onclick = clearStages;
  head.append(h, close);
  panel.appendChild(head);
  mapInstance.getContainer().appendChild(panel);
  stagesPanel = panel;
  return panel;
}

function lodgingLine(l) {
  const parts = [STAGE_KIND_LABELS[l.kind] || l.kind];
  if (l.perpendicular_dist_m >= 50) parts.push(`détour ${(l.perpendicular_dist_m / 1000).toFixed(1)} km`);
  const line = document.createElement('div');
  line.className = 'stages-lodging';
  const link = document.createElement('a');
  link.href = `https://www.openstreetmap.org/${l.osm_type}/${l.osm_id}`;
  link.target = '_blank';
  link.rel = 'noopener noreferrer';
  link.textContent = l.name;
  line.append(link, ` · ${parts.join(' · ')}`);
  if (l.phone) {
    const tel = document.createElement('a');
    tel.href = `tel:${l.phone.replace(/\s/g, '')}`;
    tel.textContent = ` ☎ ${l.phone}`;
    line.append(tel);
  }
  return line;
}

function alternativesBlock(alternatives) {
  const alt = document.createElement('details');
  const sum = document.createElement('summary');
  sum.textContent = `${alternatives.length} autre(s)`;
  alt.appendChild(sum);
  alternatives.forEach(a => alt.appendChild(lodgingLine(a)));
  return alt;
}

/** Une ligne du panneau, et son marqueur numéroté quand on y dort. */
function addStageRow(panel, { title, lodging, alternatives = [], marker, ok = true }) {
  const row = document.createElement('div');
  row.className = 'stages-row' + (ok ? '' : ' stages-out');
  const head = document.createElement('div');
  head.textContent = (ok ? '' : '⚠️ ') + title;
  row.appendChild(head);
  if (lodging) {
    const line = lodgingLine(lodging);
    line.prepend(copyButton(lodging.name), ' ');
    row.appendChild(line);
  } else {
    const none = document.createElement('div');
    none.className = 'stages-lodging';
    none.textContent = `Aucun hébergement à moins de ${STAGE_DETOUR_M / 1000} km`;
    row.appendChild(none);
  }
  if (alternatives.length > 0) row.appendChild(alternativesBlock(alternatives));
  panel.appendChild(row);

  if (lodging) {
    const el = document.createElement('div');
    el.className = 'stage-marker' + (ok ? '' : ' stages-out');
    el.textContent = marker;
    el.title = `${title} : ${lodging.name}`;
    el.onclick = () => row.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    stageMarkers.push(new maplibregl.Marker({ element: el })
      .setLngLat([lodging.lon, lodging.lat])
      .addTo(mapInstance));
  }
}

function showStages(plan) {
  const km = plan.stages.reduce((sum, s) => sum + s.distance_km, 0);
  const panel = stagesPanelShell(`${plan.num_stages} jour(s) · ${km.toFixed(0)} km · ${plan.avg_km_per_day.toFixed(1)} km/j`);
  const endpointTitle = (label, e) => e ? `${label} (${(e.distance_m / 1000).toFixed(1)} km du point)` : label;

  addStageRow(panel, {
    title: endpointTitle('Veille du départ', plan.departure),
    lodging: plan.departure?.lodging,
    alternatives: plan.departure?.alternatives,
    marker: 'D'
  });
  for (const s of plan.stages) {
    const title = `J${s.day} · ${s.distance_km.toFixed(1)} km (km ${s.start_km.toFixed(0)} → ${s.end_km.toFixed(0)})`;
    if (s.lodging) {
      addStageRow(panel, { title, lodging: s.lodging, alternatives: s.alternatives, marker: String(s.day), ok: s.within_range });
    } else {
      // Dernier jour : on dort à l'arrivée.
      addStageRow(panel, {
        title: `${title} — ${endpointTitle('arrivée', plan.arrival)}`,
        lodging: plan.arrival?.lodging,
        alternatives: plan.arrival?.alternatives,
        marker: 'A',
        ok: s.within_range
      });
    }
  }
  // Le survol marque une pause à chaque nuit.
  setFlyoverStages(plan.stages.map(s => {
    const lodging = s.lodging ?? plan.arrival?.lodging;
    return {
      endKm: s.end_km,
      title: s.lodging ? `J${s.day} · nuit` : 'Arrivée',
      detail: lodging?.name ? `hébergement : ${lodging.name}` : null
    };
  }));
}

async function planStages() {
  clearStages();
  if (!currentRouteCoords || currentRouteCoords.length < 2) {
    stagesPanelShell("Trace d'abord un itinéraire.");
    return;
  }
  stagesPanelShell('Recherche des hébergements…');
  try {
    const found = await postJson('/api/lodgings-along-route', {
      coords: currentRouteCoords.map(({ lat, lon }) => ({ lat, lon })),
      buffer_m: STAGE_DETOUR_M,
      kinds: STAGE_KINDS
    });
    if (found.count === 0) {
      stagesPanelShell(`Aucun hébergement à moins de ${STAGE_DETOUR_M / 1000} km de la trace.`);
      return;
    }
    const plan = await postJson('/api/plan-stages', {
      lodgings: found.lodgings,
      total_route_km: found.total_route_km,
      target_km_per_day: STAGE_TARGET_KM,
      min_km_per_day: STAGE_TARGET_KM - STAGE_TOLERANCE_KM,
      max_km_per_day: STAGE_TARGET_KM + STAGE_TOLERANCE_KM,
      start: currentRouteCoords[0],
      end: currentRouteCoords[currentRouteCoords.length - 1]
    });
    showStages(plan);
  } catch (e) {
    console.warn('[stages]', e.message);
    stagesPanelShell('Échec du calcul des étapes (voir la console).');
  }
}

function createStagesControl() {
  class StagesControl {
    onAdd(map) {
      this._map = map;
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';
      this._btn = document.createElement('button');
      this._btn.className = 'maplibregl-ctrl-stages';
      this._btn.textContent = '🗓️';
      this._btn.title = `Étapes d'hébergement en hébergement (${STAGE_TARGET_KM} ± ${STAGE_TOLERANCE_KM} km/j, détour ≤ ${STAGE_DETOUR_M / 1000} km)`;
      this._btn.onclick = () => (stagesPanel ? clearStages() : planStages());
      this._container.appendChild(this._btn);
      return this._container;
    }

    onRemove() {
      this._container.parentNode.removeChild(this._container);
      this._map = undefined;
    }
  }
  return new StagesControl();
}

// ── Unpaved paths overlay (Overpass API) ──────────────────────────────────
let unpavedEnabled = false;
let unpavedFetchTimer = null;
let unpavedFetching = false;

async function fetchUnpavedPaths() {
  if (!mapInstance || !unpavedEnabled) return;
  if (mapInstance.getZoom() < 11) {
    // Too zoomed out — clear and show hint
    const src = mapInstance.getSource('osm-unpaved');
    if (src) src.setData({ type: 'FeatureCollection', features: [] });
    return;
  }
  if (unpavedFetching) return;
  unpavedFetching = true;

  const b = mapInstance.getBounds();
  const bbox = `${b.getSouth().toFixed(4)},${b.getWest().toFixed(4)},${b.getNorth().toFixed(4)},${b.getEast().toFixed(4)}`;
  const query = `[out:json][timeout:20];(way["highway"~"^(track|path|footway|bridleway)$"](${bbox}););out geom qt;`;

  try {
    const res = await fetch('https://overpass-api.de/api/interpreter', {
      method: 'POST',
      body: 'data=' + encodeURIComponent(query),
      signal: AbortSignal.timeout(22000)
    });
    if (!res.ok) throw new Error('Overpass ' + res.status);
    const data = await res.json();

    const features = (data.elements || [])
      .filter(el => el.type === 'way' && el.geometry)
      .map(el => ({
        type: 'Feature',
        geometry: { type: 'LineString', coordinates: el.geometry.map(p => [p.lon, p.lat]) },
        properties: { highway: el.tags && el.tags.highway }
      }));

    const src = mapInstance.getSource('osm-unpaved');
    if (src && unpavedEnabled) src.setData({ type: 'FeatureCollection', features });
    console.log('[unpaved] loaded', features.length, 'paths');
  } catch (e) {
    console.warn('[unpaved] fetch failed:', e.message);
  } finally {
    unpavedFetching = false;
  }
}

function scheduleUnpavedFetch() {
  if (!unpavedEnabled) return;
  clearTimeout(unpavedFetchTimer);
  unpavedFetchTimer = setTimeout(fetchUnpavedPaths, 800);
}

function createUnpavedControl() {
  class UnpavedControl {
    onAdd(map) {
      this._map = map;
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';
      this._btn = document.createElement('button');
      this._btn.className = 'maplibregl-ctrl-unpaved';
      this._btn.textContent = '🟣';
      this._btn.title = 'Afficher/masquer les chemins non-goudronnés';
      this._btn.onclick = () => this.toggle();
      this._container.appendChild(this._btn);
      return this._container;
    }

    toggle() {
      unpavedEnabled = !unpavedEnabled;
      const vis = unpavedEnabled ? 'visible' : 'none';
      if (this._map.getLayer('osm-unpaved-line')) {
        this._map.setLayoutProperty('osm-unpaved-line', 'visibility', vis);
      }
      this._btn.style.backgroundColor = unpavedEnabled ? mapColor('--map-toggle-unpaved') : '';
      this._btn.style.color = unpavedEnabled ? 'white' : '';

      if (unpavedEnabled) {
        fetchUnpavedPaths();
        this._map.on('moveend', scheduleUnpavedFetch);
        this._map.on('zoomend', scheduleUnpavedFetch);
      } else {
        this._map.off('moveend', scheduleUnpavedFetch);
        this._map.off('zoomend', scheduleUnpavedFetch);
        clearTimeout(unpavedFetchTimer);
        const src = this._map.getSource('osm-unpaved');
        if (src) src.setData({ type: 'FeatureCollection', features: [] });
      }
    }

    onRemove() {
      this._container.parentNode.removeChild(this._container);
      this._map = undefined;
    }
  }
  return new UnpavedControl();
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
          pitch: 55,
          duration: 1000
        });
        this._button.classList.add('active');
        this._button.style.backgroundColor = mapColor('--map-control-active');
        this._button.style.color = mapColor('--map-marker-ink');
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
        this._button.style.backgroundColor = mapColor('--map-control-active');
        this._button.style.color = mapColor('--map-marker-ink');
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

function clearWaterMarkers() {
  waterMarkers.forEach(m => m.remove());
  waterMarkers = [];
}

function fetchWaterAlongRoute(coords) {
  if (!waterEnabled) return;
  if (!coords || coords.length === 0) return;

  let minLat = Infinity, maxLat = -Infinity, minLon = Infinity, maxLon = -Infinity;
  for (const c of coords) {
    if (c.lat < minLat) minLat = c.lat;
    if (c.lat > maxLat) maxLat = c.lat;
    if (c.lon < minLon) minLon = c.lon;
    if (c.lon > maxLon) maxLon = c.lon;
  }

  const url = `/api/pois?min_lat=${minLat}&max_lat=${maxLat}&min_lon=${minLon}&max_lon=${maxLon}`;
  fetch(url)
    .then(r => r.json())
    .then(pois => {
      clearWaterMarkers();
      const waterPois = pois.filter(p => p.poi_type === 'water');
      waterPois.forEach(poi => {
        const el = document.createElement('div');
        el.style.fontSize = '18px';
        el.style.cursor = 'pointer';
        el.style.filter = 'drop-shadow(0 1px 2px rgba(0,0,0,0.5))';
        el.textContent = '💧';

        const label = poi.name || 'Point d\'eau';
        const popup = new maplibregl.Popup({ offset: 15, closeButton: false })
          .setText(label);

        const marker = new maplibregl.Marker({ element: el, anchor: 'center' })
          .setLngLat([poi.lon, poi.lat])
          .setPopup(popup)
          .addTo(mapInstance);

        waterMarkers.push(marker);
      });
      if (waterPois.length > 0) {
        console.log(`[maplibre] ${waterPois.length} point(s) d'eau le long du parcours`);
      }
    })
    .catch(err => console.warn('[maplibre] water fetch error:', err));
}

function createWaterControl() {
  class WaterControl {
    onAdd(map) {
      this._map = map;
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';
      this._btn = document.createElement('button');
      this._btn.className = 'maplibregl-ctrl-water';
      this._btn.textContent = '💧';
      this._btn.title = 'Afficher/masquer les points d\'eau le long du parcours';
      this._btn.onclick = () => this.toggle();
      this._container.appendChild(this._btn);
      return this._container;
    }

    toggle() {
      waterEnabled = !waterEnabled;
      this._btn.style.backgroundColor = waterEnabled ? mapColor('--map-toggle-water') : '';
      this._btn.style.color = waterEnabled ? 'white' : '';

      if (waterEnabled) {
        fetchWaterAlongRoute(currentRouteCoords);
      } else {
        clearWaterMarkers();
      }
    }

    onRemove() {
      this._container.parentNode.removeChild(this._container);
      this._map = undefined;
    }
  }
  return new WaterControl();
}

function createFullscreenControl() {
  class FullscreenCtrl {
    onAdd(map) {
      this._map = map;
      this._container = document.createElement('div');
      this._container.className = 'maplibregl-ctrl maplibregl-ctrl-group';
      this._button = document.createElement('button');
      this._button.textContent = '⛶';
      this._button.title = 'Plein écran';
      this._button.onclick = () => this.toggle();
      this._container.appendChild(this._button);

      // Échap pour quitter le plein écran
      this._escHandler = (e) => {
        if (e.key === 'Escape' && this._active) this.toggle();
      };
      document.addEventListener('keydown', this._escHandler);
      this._active = false;
      return this._container;
    }
    toggle() {
      const mapEl = document.getElementById('map');
      if (!mapEl) return;
      this._active = mapEl.classList.toggle('map-fullscreen');
      this._button.textContent = this._active ? '✕' : '⛶';
      this._button.title = this._active ? 'Quitter le plein écran' : 'Plein écran';
      setTimeout(() => { mapInstance?.resize(); }, 50);
    }
    onRemove() {
      document.removeEventListener('keydown', this._escHandler);
      this._container.parentNode?.removeChild(this._container);
      this._map = undefined;
    }
  }
  return new FullscreenCtrl();
}


// --- Noms de lieux (clic droit) -------------------------------------------
// Les noms affichés par les fonds IGN sont dans l'image des tuiles et ne se
// sélectionnent pas. Le géocodage inverse IGN ne renvoie que des communes ;
// les lieux-dits, hameaux, crêtes et cols viennent de la BD TOPO en WFS.
const IGN_WFS_URL = 'https://data.geopf.fr/wfs/ows';
const TOPONYM_LAYERS = [
  'BDTOPO_V3:lieu_dit_non_habite',
  'BDTOPO_V3:zone_d_habitation',
  'BDTOPO_V3:detail_orographique'
];
const TOPONYM_RADIUS_M = 1500;
const TOPONYM_MAX = 4;
const RIGHT_DRAG_TOLERANCE_PX = 4;

function distanceM(lat1, lon1, lat2, lon2) {
  const rad = Math.PI / 180;
  const x = (lon2 - lon1) * rad * Math.cos(((lat1 + lat2) / 2) * rad);
  const y = (lat2 - lat1) * rad;
  return Math.sqrt(x * x + y * y) * 6371000;
}

// Premier sommet suffit : les zones d'habitation sont petites devant le rayon.
function firstCoord(geometry) {
  let c = geometry?.coordinates;
  while (Array.isArray(c) && Array.isArray(c[0])) c = c[0];
  return Array.isArray(c) ? c : null;
}

async function fetchToponyms(lat, lon) {
  const dLat = TOPONYM_RADIUS_M / 111320;
  const dLon = dLat / Math.cos((lat * Math.PI) / 180);
  const bbox = [lat - dLat, lon - dLon, lat + dLat, lon + dLon].join(',');
  const requests = TOPONYM_LAYERS.map(async (layer) => {
    const params = new URLSearchParams({
      service: 'WFS',
      version: '2.0.0',
      request: 'GetFeature',
      typeNames: layer,
      outputFormat: 'application/json',
      srsName: 'EPSG:4326',
      count: '50',
      bbox: bbox + ',urn:ogc:def:crs:EPSG::4326'
    });
    const res = await fetch(IGN_WFS_URL + '?' + params);
    if (!res.ok) throw new Error('WFS ' + res.status);
    return (await res.json()).features || [];
  });
  const byName = new Map();
  for (const result of await Promise.allSettled(requests)) {
    if (result.status !== 'fulfilled') continue;
    for (const f of result.value) {
      const name = f.properties?.toponyme;
      const c = firstCoord(f.geometry);
      if (!name || !c) continue;
      const d = distanceM(lat, lon, c[1], c[0]);
      const prev = byName.get(name);
      if (!prev || d < prev.d) byName.set(name, { name, nature: f.properties.nature, d });
    }
  }
  return [...byName.values()].sort((a, b) => a.d - b.d).slice(0, TOPONYM_MAX);
}

async function fetchCommune(lat, lon) {
  const url = `https://data.geopf.fr/geocodage/reverse?lat=${lat}&lon=${lon}&index=poi&limit=1`;
  const res = await fetch(url);
  if (!res.ok) return null;
  const f = (await res.json()).features?.[0];
  return f?.properties?.toponym || null;
}

function copyButton(text) {
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'place-copy';
  btn.textContent = '📋';
  btn.title = 'Copier « ' + text + ' »';
  btn.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(text);
      btn.textContent = '✓';
    } catch (err) {
      console.warn('[maplibre] copie impossible', err);
      btn.textContent = '✗';
    }
    setTimeout(() => { btn.textContent = '📋'; }, 1200);
  });
  return btn;
}

// Pas d'API publique Airbnb/Booking : on ouvre leur recherche centrée sur le point.
const LODGING_RADIUS_KM = 10;

function lodgingLinks(lat, lon) {
  const dLat = LODGING_RADIUS_KM / 111;
  const dLon = LODGING_RADIUS_KM / (111 * Math.cos((lat * Math.PI) / 180));
  const airbnb = 'https://www.airbnb.fr/s/homes?' + new URLSearchParams({
    ne_lat: (lat + dLat).toFixed(5), ne_lng: (lon + dLon).toFixed(5),
    sw_lat: (lat - dLat).toFixed(5), sw_lng: (lon - dLon).toFixed(5),
    search_by_map: 'true'
  });
  const booking = 'https://www.booking.com/searchresults.fr.html?' + new URLSearchParams({
    latitude: lat.toFixed(5), longitude: lon.toFixed(5),
    radius: String(LODGING_RADIUS_KM), order: 'distance_from_search'
  });
  const row = document.createElement('div');
  row.className = 'place-row place-lodging';
  row.append(`Dormir (${LODGING_RADIUS_KM} km) :`);
  for (const [label, href] of [['Airbnb', airbnb], ['Booking', booking]]) {
    const a = document.createElement('a');
    a.href = href;
    a.target = '_blank';
    a.rel = 'noopener noreferrer';
    a.textContent = label;
    row.append(' ', a);
  }
  return row;
}

function placeRow(label, detail, text) {
  const row = document.createElement('div');
  row.className = 'place-row';
  const span = document.createElement('span');
  span.className = 'place-name';
  span.textContent = label;
  if (detail) {
    const small = document.createElement('small');
    small.textContent = ' ' + detail;
    span.appendChild(small);
  }
  row.append(span, copyButton(text));
  return row;
}

let placePopup = null;

async function showPlaceNames(lngLat) {
  const { lat, lng } = lngLat;
  const box = document.createElement('div');
  box.className = 'place-popup';
  box.textContent = 'Recherche des noms…';
  placePopup?.remove();
  const popup = new maplibregl.Popup({ offset: 8, maxWidth: '280px' })
    .setLngLat(lngLat)
    .setDOMContent(box)
    .addTo(mapInstance);
  placePopup = popup;
  popup.on('close', () => {
    if (placePopup === popup) placePopup = null;
  });

  const [toponyms, commune] = await Promise.all([
    fetchToponyms(lat, lng).catch(() => []),
    fetchCommune(lat, lng).catch(() => null)
  ]);
  if (placePopup !== popup) return;

  box.textContent = '';
  for (const t of toponyms) {
    box.appendChild(placeRow(t.name, `${t.nature} · ${Math.round(t.d)} m`, t.name));
  }
  if (commune) box.appendChild(placeRow(commune, 'commune', commune));
  if (toponyms.length === 0 && !commune) {
    const empty = document.createElement('div');
    empty.textContent = 'Aucun nom trouvé à proximité.';
    box.appendChild(empty);
  }
  const coords = `${lat.toFixed(5)}, ${lng.toFixed(5)}`;
  box.appendChild(placeRow(coords, null, coords));
  box.appendChild(lodgingLinks(lat, lng));
}

function setupPlaceNames() {
  // Clic droit glissé = rotation de la carte : on n'ouvre la bulle que si la
  // souris n'a pas bougé entre l'appui et le relâchement.
  let downAt = null;
  mapInstance.getCanvas().addEventListener('mousedown', (e) => {
    if (e.button === 2) downAt = { x: e.clientX, y: e.clientY };
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') placePopup?.remove();
  });
  mapInstance.on('contextmenu', (event) => {
    const e = event.originalEvent;
    if (downAt && e &&
        Math.hypot(e.clientX - downAt.x, e.clientY - downAt.y) > RIGHT_DRAG_TOLERANCE_PX) {
      return;
    }
    e?.preventDefault();
    showPlaceNames(event.lngLat);
  });
}


export function initMap() {
  ensureMap();
  if (!clickHandlerSet) {
    // Couches du tracé courant, des plus larges aux plus fines. Les halos sont
    // inclus volontairement : ils doublent la cible sans rien changer au point
    // renvoyé, et viser une ligne de 4 px à la souris n'est pas raisonnable.
    const LIGNES_DU_TRACE = [
      'route-line-outline',
      'route-line',
      'route-line-unpaved-outline',
      'route-line-unpaved'
    ];
    const TOLERANCE_PX = 8;

    mapInstance.on('click', (event) => {
      // Bulle des noms ouverte : le clic la ferme sans poser de point.
      if (placePopup) {
        placePopup.remove();
        return;
      }
      // Clic sur un hébergement : sa bulle s'ouvre (handler de la couche), pas de point.
      if (
        mapInstance.getLayer('lodging-points') &&
        mapInstance.queryRenderedFeatures(event.point, { layers: ['lodging-points'] }).length > 0
      ) {
        return;
      }
      const { lng, lat } = event.lngLat;
      const couches = LIGNES_DU_TRACE.filter((id) => mapInstance.getLayer(id));
      const p = event.point;
      const touche =
        couches.length > 0 &&
        mapInstance.queryRenderedFeatures(
          [
            [p.x - TOLERANCE_PX, p.y - TOLERANCE_PX],
            [p.x + TOLERANCE_PX, p.y + TOLERANCE_PX]
          ],
          { layers: couches }
        ).length > 0;

      const nom = touche ? 'route-line-click' : 'map-click';
      console.debug('[maplibre] ' + nom, { lat, lon: lng });
      window.dispatchEvent(new CustomEvent(nom, { detail: { lat, lon: lng } }));
    });
    setupPlaceNames();
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

// Latest surface flags, set by setRouteSurfaces port (null = no data)
let currentRouteSurfaces = null;
// Points suivant un chemin IGN absent d'OSM (null = aucun)
let currentRouteIgn = null;

let currentDirectOverlay = null;

/** Tronçons directs ({ lines, obstacles }) envoyés par Elm, redessinés quand la carte est prête. */
export function setDirectOverlay(overlay) {
  currentDirectOverlay = overlay;
  _applyDirectOverlay();
}

function _applyDirectOverlay() {
  if (!mapInstance) return;
  const lineSrc = mapInstance.getSource('route-direct');
  const dotSrc = mapInstance.getSource('route-obstacles');
  if (!lineSrc || !dotSrc) return;

  const overlay = currentDirectOverlay || { lines: [], obstacles: [] };
  lineSrc.setData({
    type: 'FeatureCollection',
    features: overlay.lines
      .filter(line => line.length >= 2)
      .map(line => ({
        type: 'Feature',
        geometry: { type: 'LineString', coordinates: line.map(c => [c.lon, c.lat]) },
        properties: {}
      }))
  });
  dotSrc.setData({
    type: 'FeatureCollection',
    features: overlay.obstacles.map(o => ({
      type: 'Feature',
      geometry: { type: 'Point', coordinates: [o.lon, o.lat] },
      properties: { kind: o.kind }
    }))
  });
}

export function setRouteSurfaces({ surfaces, ign, elevations }) {
  setFlyoverElevations(elevations);
  currentRouteSurfaces = surfaces; // null or Array<bool>
  currentRouteIgn = ign;
  if (ign) {
    console.log('[maplibre] setRouteSurfaces', ign.filter(Boolean).length, 'pts sur chemin IGN');
  }
  if (surfaces) {
    const unpavedCount = surfaces.filter(s => s === false).length;
    console.log('[maplibre] setRouteSurfaces', surfaces.length, 'pts,', unpavedCount, 'unpaved');
  } else {
    console.log('[maplibre] setRouteSurfaces null');
  }
  _applyUnpavedOverlay();
  _applyIgnOverlay();
}

/** Lignes formées des points consécutifs pour lesquels `flags[i]` est vrai. */
function _runsWhere(flags) {
  const features = [];
  let seg = null;
  for (let i = 0; i < currentRoute.length; i++) {
    const point = [currentRoute[i].lon, currentRoute[i].lat];
    if (flags[i]) {
      // Le point d'avant ouvre la ligne, pour qu'elle rejoigne le reste du tracé.
      if (!seg) seg = i > 0 ? [[currentRoute[i - 1].lon, currentRoute[i - 1].lat], point] : [point];
      else seg.push(point);
    } else if (seg) {
      seg.push(point);
      if (seg.length >= 2) features.push(seg);
      seg = null;
    }
  }
  if (seg && seg.length >= 2) features.push(seg);
  return features.map(coordinates => ({ type: 'Feature', geometry: { type: 'LineString', coordinates }, properties: {} }));
}

function _applyIgnOverlay() {
  if (!mapInstance) return;
  const src = mapInstance.getSource('route-ign');
  if (!src) return;
  const features = currentRouteIgn && currentRoute ? _runsWhere(currentRouteIgn) : [];
  src.setData({ type: 'FeatureCollection', features });
}

function _applyUnpavedOverlay() {
  if (!mapInstance) return;
  const src = mapInstance.getSource('route-unpaved');
  if (!src) return;

  if (!currentRouteSurfaces || !currentRoute || currentRoute.length === 0) {
    src.setData({ type: 'FeatureCollection', features: [] });
    return;
  }

  // Build multi-linestring from consecutive unpaved (false) points
  const features = [];
  let seg = null;

  for (let i = 0; i < currentRoute.length; i++) {
    const paved = currentRouteSurfaces[i] !== false; // treat missing as paved
    if (!paved) {
      if (!seg) seg = [[currentRoute[i].lon, currentRoute[i].lat]];
      else seg.push([currentRoute[i].lon, currentRoute[i].lat]);
      // Connect at boundaries: include last paved point as segment start
    } else {
      if (seg && seg.length >= 2) {
        features.push({ type: 'Feature', geometry: { type: 'LineString', coordinates: seg }, properties: {} });
      }
      // Start next segment at current paved point so segments connect cleanly
      seg = [[currentRoute[i].lon, currentRoute[i].lat]];
    }
  }
  if (seg && seg.length >= 2) {
    features.push({ type: 'Feature', geometry: { type: 'LineString', coordinates: seg }, properties: {} });
  }
  src.setData({ type: 'FeatureCollection', features });
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
    setFlyoverRoute([]);
    currentRouteSurfaces = null;
    currentRouteIgn = null;
    _applyUnpavedOverlay();
    _applyIgnOverlay();
    currentDirectOverlay = null;
    _applyDirectOverlay();
    // Clear km markers when route is cleared
    kmMarkers.forEach(m => m.remove());
    kmMarkers = [];
    currentRouteCoords = null;
    clearWaterMarkers();
    clearStages();
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
  _applyUnpavedOverlay();
  _applyIgnOverlay();

  setFlyoverRoute(coords);

  // Km markers désactivés sur demande utilisateur — encombraient le circuit.
  // Le code de updateKmMarkers reste disponible si on veut les réactiver via
  // une option future. On nettoie ici les markers existants au cas où.
  kmMarkers.forEach(m => m.remove());
  kmMarkers = [];

  currentRouteCoords = coords;
  fetchWaterAlongRoute(coords);
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
      // Pastille neutre cerclée de couleur : le trait de la route change de
      // teinte selon le revêtement, un marqueur plein lui ferait concurrence.
      el.style.backgroundColor = mapColor('--map-marker-face');
      el.style.border =
        '2px solid ' + mapColor(type === 'start' ? '--map-ring-start' : '--map-ring-end');
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

/**
 * Point de position sur le tracé, piloté par la bande de dénivelé.
 * `coord` vaut null quand il n'y a plus rien à montrer.
 */
export function setElevationHoverMarker(coord) {
  ensureMap();

  if (!coord || typeof coord.lat !== 'number' || typeof coord.lon !== 'number') {
    if (elevationMarker) {
      elevationMarker.remove();
      elevationMarker = null;
    }
    return;
  }

  if (!elevationMarker) {
    const el = document.createElement('div');
    el.className = 'elevation-hover-marker';
    elevationMarker = new maplibregl.Marker({ element: el })
      .setLngLat([coord.lon, coord.lat])
      .addTo(mapInstance);
  } else {
    elevationMarker.setLngLat([coord.lon, coord.lat]);
  }
}

let currentReferenceRoutes = [];

/**
 * Itinéraires en surimpression ([{ path, color }]), liste vide pour tout effacer.
 * Redessinés aussi quand la carte finit de charger, s'ils arrivent avant.
 */
export function setReferenceRoutes(routes) {
  ensureMap();
  const added = routes.length > currentReferenceRoutes.length;
  currentReferenceRoutes = routes;
  if (_applyReferenceRoutes() && added) {
    _frameReferenceRoutes();
  }
}

function _applyReferenceRoutes() {
  const source = mapInstance && mapInstance.getSource('reference');
  if (!source) return false;

  source.setData({
    type: 'FeatureCollection',
    features: currentReferenceRoutes
      .filter((route) => route.path.length >= 2)
      .map((route) => ({
        type: 'Feature',
        properties: { color: route.color },
        geometry: { type: 'LineString', coordinates: route.path.map((c) => [c.lon, c.lat]) }
      }))
  });
  return true;
}

// Cadrer sur l'ensemble calques + tracé courant. Un calque déborde presque
// toujours de l'itinéraire affiché — un GR entier contre une de ses
// sections — et rien ne signalait qu'il continuait hors écran.
function _frameReferenceRoutes() {
  const points = currentReferenceRoutes.flatMap((route) => route.path.map((c) => [c.lon, c.lat]));

  const route = mapInstance.getSource('route');
  const routeData = route && (route.serialize ? route.serialize().data : route._data);
  const routeLine = routeData
    && routeData.features
    && routeData.features[0]
    && routeData.features[0].geometry.coordinates;
  if (routeLine) {
    points.push(...routeLine);
  }
  if (points.length === 0) return;

  const bounds = new maplibregl.LngLatBounds(points[0], points[0]);
  points.forEach((point) => bounds.extend(point));
  mapInstance.fitBounds(bounds, { padding: margesDesCalques(), duration: 800 });
}

export function toggleSatelliteView(enabled) {
  // Legacy compatibility: true → satellite, false → topo
  switchMapStyle(enabled ? 'satellite' : 'topo');
}

/**
 * Switch map style between topo, satellite, and hybrid.
 * @param {string} style - 'topo' | 'satellite' | 'hybrid'
 */
/** Create a raster layer below the OSM one, the first time it is needed. */
function ensureRasterLayer(layerId, sourceId, needed) {
  if (!needed || mapInstance.getLayer(layerId)) return;
  mapInstance.addLayer(
    { id: layerId, type: 'raster', source: sourceId, minzoom: 0, maxzoom: 22 },
    'osm-tiles'
  );
}

function setLayerVisible(layerId, visible) {
  if (!mapInstance.getLayer(layerId)) return;
  mapInstance.setLayoutProperty(layerId, 'visibility', visible ? 'visible' : 'none');
}

let ignVectorLayerIds = []; // Injected Plan IGN vector layers, empty until loaded
let ignVectorMeta = {}; // id de couche -> { sourceLayer, symbo, role }
let ignPeintureOrigine = {}; // id -> peinture d'origine, pour revenir en arrière
let ignVectorPromise = null;

/** Pull the Plan IGN vector style into the running map.
 *
 * Raster tiles are frozen at 256px; the vector plan redraws at any zoom and any
 * pixel density, which is what "sharp" actually means here. We inject the style's
 * layers into our existing map rather than calling setStyle, which would wipe the
 * route, terrain and marker layers.
 *
 * The raster Plan IGN stays as the fallback if this fails.
 */
/* Les propriétés à relever dépendent du type de couche : demander `fill-color`
   à une couche `line` ne renvoie pas `undefined`, ça lève une exception au
   fond de MapLibre. Mesuré ici, et la facture était lourde : l'exception
   remontait jusqu'au `.catch` du chargement vectoriel, qui concluait que le
   Plan IGN était indisponible et rebasculait les 425 couches sur le raster.
   Un relevé de trop faisait perdre tout le style. */
const PROPS_PAR_TYPE = {
  line: ['line-color', 'line-width', 'line-opacity', 'line-dasharray'],
  fill: ['fill-color', 'fill-opacity'],
  symbol: [],
  circle: ['circle-color', 'circle-radius']
};

function peintureOrigine(id, type) {
  if (!ignPeintureOrigine[id]) {
    const garde = {};
    (PROPS_PAR_TYPE[type] || []).forEach((prop) => {
      try {
        garde[prop] = mapInstance.getPaintProperty(id, prop);
      } catch (e) {
        /* propriété absente de ce type de couche */
      }
    });
    ignPeintureOrigine[id] = garde;
  }
  return ignPeintureOrigine[id];
}

/**
 * Applique ou retire l'inversion sur les couches IGN déjà injectées.
 *
 * On repeint les mêmes couches plutôt que d'en injecter 425 de plus : passer
 * de « Plan IGN » à « Chemin noir » devient un changement de peinture, pas un
 * rechargement de style.
 */
function appliquerCheminNoir(actif) {
  if (ignVectorLayerIds.length === 0) return;

  const encreChemin = mapColor('--map-chemin-ink');
  const paleBitume = mapColor('--map-bitume-pale');

  ignVectorLayerIds.forEach((id) => {
    const meta = ignVectorMeta[id];
    if (!meta) return;
    const caste = casteIgn(meta);
    if (caste === 'nulle') return;

    const origine = peintureOrigine(id, meta.type);

    if (!actif) {
      Object.entries(origine).forEach(([prop, valeur]) => {
        try {
          mapInstance.setPaintProperty(id, prop, valeur);
        } catch (e) {
          /* propriété absente de ce type de couche */
        }
      });
      setLayerVisible(id, true);
      return;
    }

    // Les « filets extérieurs » sont les contours qui donnent du poids à une
    // route. C'est exactement le poids qu'on lui retire.
    if (caste === 'cicatrice' && meta.role === 'ext') {
      setLayerVisible(id, false);
      return;
    }
    setLayerVisible(id, true);

    if (meta.type !== 'line') {
      if (caste === 'cicatrice' && origine['fill-color'] !== undefined) {
        mapInstance.setPaintProperty(id, 'fill-color', paleBitume);
      }
      return;
    }

    if (caste === 'matiere') {
      mapInstance.setPaintProperty(id, 'line-color', encreChemin);
      mapInstance.setPaintProperty(id, 'line-opacity', 1);
      // Un tireté se lit comme « incertain ». Un chemin noir ne l'est pas.
      mapInstance.setPaintProperty(id, 'line-dasharray', undefined);
      const largeur = largeurMiseALEchelle(origine['line-width'], 3.5, 1.6, 11);
      if (largeur !== undefined) mapInstance.setPaintProperty(id, 'line-width', largeur);
    } else {
      mapInstance.setPaintProperty(id, 'line-color', paleBitume);
      mapInstance.setPaintProperty(id, 'line-opacity', 1);
      const largeur = largeurMiseALEchelle(origine['line-width'], 1 / 6, 0.5, 2);
      if (largeur !== undefined) mapInstance.setPaintProperty(id, 'line-width', largeur);
    }
  });

  // Le tracé calculé doit rester au-dessus de tout, y compris des chemins
  // qu'on vient d'épaissir.
  ensureRouteLayers();
}

async function loadIgnVectorLayers() {
  const style = await fetch(IGN_VECTOR_STYLE_URL).then((r) => {
    if (!r.ok) throw new Error('style HTTP ' + r.status);
    return r.json();
  });

  if (style.sprite) mapInstance.setSprite(style.sprite);

  const source = style.sources && style.sources.plan_ign;
  if (!source) throw new Error('plan_ign source missing from style');
  if (!mapInstance.getSource('plan_ign')) {
    mapInstance.addSource('plan_ign', { ...source, attribution: IGN_ATTRIBUTION });
  }

  // Below the hillshade so the relief still reads through the plan.
  const before = mapInstance.getLayer('hills')
    ? 'hills'
    : mapInstance.getLayer('osm-tiles')
      ? 'osm-tiles'
      : undefined;

  const ids = [];
  for (const layer of style.layers) {
    const id = 'ignv-' + layer.id;
    ignVectorMeta[id] = {
      sourceLayer: layer['source-layer'],
      symbo: symboDuFiltre(layer.filter),
      role: /\bext/i.test(layer.id) ? 'ext' : /\bint/i.test(layer.id) ? 'int' : 'autre',
      type: layer.type
    };
    if (!mapInstance.getLayer(id)) {
      mapInstance.addLayer(
        { ...layer, id, layout: { ...(layer.layout || {}), visibility: 'none' } },
        before
      );
    }
    ids.push(id);
  }

  ignVectorLayerIds = ids;
  ensureRouteLayers();
  console.debug('[maplibre] Plan IGN vector: ' + ids.length + ' layers injected');
}

function setIgnVectorVisible(visible) {
  ignVectorLayerIds.forEach((id) => setLayerVisible(id, visible));
}

/** Les deux styles qui s'appuient sur le Plan IGN vectoriel. */
function estPlanVectoriel(style) {
  return style === 'ign-plan' || style === 'chemin-noir';
}

/** Show the Plan IGN, vector if we can get it, raster meanwhile. */
function applyIgnPlan(wanted) {
  if (wanted && !ignVectorPromise) {
    ignVectorPromise = loadIgnVectorLayers()
      .then(() => {
        applyIgnPlan(estPlanVectoriel(currentMapStyle));
        // Les couches arrivent après coup : si « Chemin noir » était déjà
        // demandé, c'est ici qu'il faut repeindre. Isolé dans son propre
        // `try` : un défaut de repeinte doit coûter l'inversion, pas le fond
        // de carte entier.
        try {
          appliquerCheminNoir(currentMapStyle === 'chemin-noir');
        } catch (err) {
          console.warn('[maplibre] inversion Chemin noir impossible', err);
        }
      })
      .catch((err) => {
        console.warn('[maplibre] Plan IGN vector unavailable, keeping raster', err);
        ignVectorLayerIds = [];
        applyIgnPlan(estPlanVectoriel(currentMapStyle));
      });
  }

  const vectorReady = ignVectorLayerIds.length > 0;
  setIgnVectorVisible(wanted && vectorReady);
  // Raster only fills in while the vector layers are not there.
  ensureRasterLayer('ign-plan-tiles', 'ign-plan', wanted && !vectorReady);
  setLayerVisible('ign-plan-tiles', wanted && !vectorReady);
}

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

  // Same lazy pattern for the IGN backgrounds: created on first use, below OSM.
  ensureRasterLayer('ign-ortho-tiles', 'ign-ortho', style === 'ign-ortho');

  // « Chemin noir » est le Plan IGN vectoriel repeint : mêmes couches, mêmes
  // tuiles, hiérarchie retournée.
  const isIgn = style === 'ign-plan' || style === 'ign-ortho' || style === 'chemin-noir';
  const showOsm = style === 'topo' || style === 'hybrid';
  const showSatellite = style === 'satellite' || style === 'hybrid';

  // Set satellite visibility
  if (mapInstance.getLayer('satellite-tiles')) {
    mapInstance.setLayoutProperty('satellite-tiles', 'visibility', showSatellite ? 'visible' : 'none');
  }

  applyIgnPlan(style === 'ign-plan' || style === 'chemin-noir');
  appliquerCheminNoir(style === 'chemin-noir');
  setLayerVisible('ign-ortho-tiles', style === 'ign-ortho');

  // Set OSM visibility and opacity
  if (mapInstance.getLayer('osm-tiles')) {
    mapInstance.setLayoutProperty('osm-tiles', 'visibility', showOsm ? 'visible' : 'none');
    // In hybrid mode, make OSM semi-transparent on top of satellite
    mapInstance.setPaintProperty('osm-tiles', 'raster-opacity', style === 'hybrid' ? 0.45 : 1);
  }

  // Set hillshade visibility — hide in pure satellite, show in topo/hybrid
  if (mapInstance.getLayer('hills')) {
    const flatBackground = style === 'satellite' || style === 'ign-ortho';
    mapInstance.setLayoutProperty('hills', 'visibility', flatBackground ? 'none' : 'visible');
  }

  // Set contour lines visibility — show in topo and 3D, hide in pure satellite
  // Plan IGN already draws its own contour lines — ours would double them up.
  const showContours = style !== 'satellite' && !isIgn;
  if (mapInstance.getLayer('contour-lines')) {
    mapInstance.setLayoutProperty('contour-lines', 'visibility', showContours ? 'visible' : 'none');
  }
  if (mapInstance.getLayer('contour-labels')) {
    mapInstance.setLayoutProperty('contour-labels', 'visibility', showContours ? 'visible' : 'none');
  }

  // Adjust terrain exaggeration for satellite mode (more dramatic relief)
  if (terrainEnabled || mapInstance.getTerrain()) {
    const exag = (style === 'satellite' || style === 'ign-ortho') ? 1.8 : TERRAIN_EXAGGERATION;
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
        'line-color': mapColor('--map-bbox'),
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

/** Survol 3D, piloté aussi par Elm (ports startAnimation / stopAnimation). */
export function startAnimation() {
  ensureMap();
  startFlyover();
}

export function stopAnimation() {
  stopFlyover();
}

function enableTerrainForFlyover() {
  if (terrainEnabled) return;
  mapInstance.setTerrain({ source: 'terrainSource', exaggeration: TERRAIN_EXAGGERATION });
  terrainEnabled = true;
}

export function updateWaypointMarkers(waypoints) {
  setFlyoverWaypoints(waypoints);
  // Remove all existing waypoint markers
  waypointMarkers.forEach(marker => marker.remove());
  waypointMarkers = [];

  if (!Array.isArray(waypoints) || !mapInstance) {
    return;
  }

  // Create numbered, draggable markers with delete button
  waypoints.forEach((coord, index) => {
    const displayCoord = coord;
    // Circle element = marker root (no wrapper div to avoid anchor miscalculation)
    const el = document.createElement('div');
    el.style.boxSizing = 'border-box';
    el.style.width = '28px';
    el.style.height = '28px';
    el.style.borderRadius = '50%';
    el.style.backgroundColor = mapColor('--map-marker-face');
    el.style.border = '2px solid ' + mapColor('--map-ring-waypoint');
    el.style.display = 'flex';
    el.style.alignItems = 'center';
    el.style.justifyContent = 'center';
    el.style.fontSize = '12px';
    el.style.fontWeight = 'bold';
    el.style.color = mapColor('--map-marker-ink');
    el.style.cursor = 'grab';
    el.style.boxShadow = '0 2px 6px rgba(0,0,0,0.3)';
    // In game mode: first waypoint is "D" (départ), rest are balise numbers
    // In planning mode: simple 1, 2, 3...
    el.textContent = (index + 1).toString();

    // Delete button (× top-right, visible on hover)
    const delBtn = document.createElement('div');
    delBtn.style.position = 'absolute';
    delBtn.style.top = '-8px';
    delBtn.style.right = '-8px';
    delBtn.style.width = '18px';
    delBtn.style.height = '18px';
    delBtn.style.borderRadius = '50%';
    delBtn.style.backgroundColor = mapColor('--map-ring-end');
    delBtn.style.border = '1.5px solid ' + mapColor('--map-marker-face');
    delBtn.style.display = 'none';
    delBtn.style.alignItems = 'center';
    delBtn.style.justifyContent = 'center';
    delBtn.style.fontSize = '11px';
    delBtn.style.fontWeight = 'bold';
    delBtn.style.color = mapColor('--map-marker-face');
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

// Force MapLibre to recompute its viewport — used after a CSS-driven size change
// (ex : toggle plein écran). Sans ça la map garde son ancien viewport.
export function resizeMap() {
  if (!mapInstance) return;
  mapInstance.resize();
}

/** Rayon de contexte cadré autour d'un lieu recherché, en kilomètres. */
const RAYON_RECHERCHE_KM = 5;

/**
 * Marges à laisser libres : la carte occupe toute la fenêtre, mais le panneau
 * et la bande de dénivelé sont posés dessus. Cadrer sans les déduire plaçait
 * le lieu au centre géométrique de la carte, c'est-à-dire derrière le panneau
 * de 480 px. On mesure le rendu plutôt que de relire `--panel-width` : le
 * panneau est redimensionnable à la souris, et replié il sort de l'écran sans
 * que la variable change.
 */
function margesDesCalques() {
  const marges = { top: 40, right: 40, bottom: 40, left: 40 };
  const visible = (el) => el && el.getBoundingClientRect().width > 0
    && getComputedStyle(el).visibility !== 'hidden';

  const panneau = document.querySelector('.app-container');
  if (visible(panneau)) {
    marges.left = panneau.getBoundingClientRect().right + 24;
  }
  const bande = document.querySelector('.elevation-band');
  if (visible(bande)) {
    marges.bottom = bande.getBoundingClientRect().height + 24;
  }
  return marges;
}

/**
 * Cadre un lieu avec son contexte alentour.
 *
 * `jumpTo({ center })` ne touchait pas au zoom : depuis la vue France on
 * restait à z5, recentré mais sans rien voir de plus. On cadre donc une boîte
 * de `rayonKm` autour du point, et `fitBounds` en déduit le zoom que le
 * viewport réel permet — un zoom en dur mentirait sur un écran étroit.
 */
export function centerMapOnCoord(lat, lon, rayonKm = RAYON_RECHERCHE_KM) {
  if (!mapInstance) return;
  // Un degré de latitude vaut ~111,32 km partout ; en longitude ce pas rétrécit
  // avec le cosinus de la latitude — à 45° il ne fait plus que 79 km.
  const dLat = rayonKm / 111.32;
  const dLon = rayonKm / (111.32 * Math.cos((lat * Math.PI) / 180));
  mapInstance.fitBounds(
    [[lon - dLon, lat - dLat], [lon + dLon, lat + dLat]],
    { padding: margesDesCalques(), duration: 800, maxZoom: 15 }
  );
}
