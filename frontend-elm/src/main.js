/**
 * Glue code Elm ↔ MapLibre GL JS
 * Connecte l'application Elm avec la carte MapLibre via les Ports
 */

import maplibregl from 'maplibre-gl';

// Import de l'application Elm compilée
import { Elm } from './Main.elm';

// Import des fonctions MapLibre depuis le fichier original
// Note: Ce fichier doit être copié depuis frontend/maplibre_map.js
import * as MapLibreMap from './maplibre_map.js';

// Initialiser l'application Elm
const app = Elm.Main.init({
  node: document.getElementById('app')
});

// ============================================================
// PORTS OUT : Elm → JavaScript
// ============================================================

// Initialiser la carte
app.ports.initMap.subscribe(() => {
  console.log('[Elm→JS] initMap');
  MapLibreMap.initMap();
});

// Mettre à jour la route affichée
app.ports.updateRoute.subscribe((coords) => {
  console.log('[Elm→JS] updateRoute', coords.length, 'points');
  MapLibreMap.updateRoute(coords);
});

// Mettre à jour les marqueurs de waypoints
app.ports.updateWaypointMarkers.subscribe((waypoints) => {
  console.log('[Elm→JS] updateWaypointMarkers', waypoints.length, 'waypoints');
  MapLibreMap.updateWaypointMarkers(waypoints);
});

// Switch map style: topo / satellite / hybrid
app.ports.switchMapStyle.subscribe((style) => {
  console.log('[Elm→JS] switchMapStyle', style);
  MapLibreMap.switchMapStyle(style);
});

// Basculer vue 3D/2D
app.ports.toggleThree3DView.subscribe((enabled) => {
  console.log('[Elm→JS] toggleThree3DView', enabled);
  MapLibreMap.toggleThree3DView(enabled);
});

// Mettre à jour la bounding box
app.ports.updateBbox.subscribe((bounds) => {
  console.log('[Elm→JS] updateBbox', bounds);
  MapLibreMap.updateBbox(bounds);
});

// Centrer la carte sur les marqueurs
app.ports.centerOnMarkers.subscribe(({ start, end }) => {
  console.log('[Elm→JS] centerOnMarkers', { start, end });
  MapLibreMap.centerOnMarkers(start, end);
});

// Démarrer l'animation
app.ports.startAnimation.subscribe(() => {
  console.log('[Elm→JS] startAnimation');
  MapLibreMap.startAnimation();
});

// Arrêter l'animation
app.ports.stopAnimation.subscribe(() => {
  console.log('[Elm→JS] stopAnimation');
  MapLibreMap.stopAnimation();
});

// Sauvegarder une route dans localStorage
app.ports.saveRouteToLocalStorage.subscribe((routeData) => {
  console.log('[Elm→JS] saveRouteToLocalStorage');
  try {
    localStorage.setItem('chemins-noirs-saved-route', JSON.stringify(routeData));
    console.log('✅ Route sauvegardée dans localStorage');
  } catch (error) {
    console.error('❌ Erreur lors de la sauvegarde:', error);
  }
});

// Charger une route depuis localStorage
app.ports.loadRouteFromLocalStorage.subscribe(() => {
  console.log('[Elm→JS] loadRouteFromLocalStorage');
  try {
    const savedRoute = localStorage.getItem('chemins-noirs-saved-route');
    if (savedRoute) {
      const routeData = JSON.parse(savedRoute);
      app.ports.routeLoadedFromLocalStorage.send(routeData);
      console.log('✅ Route chargée depuis localStorage');
    } else {
      console.log('ℹ️ Aucune route sauvegardée trouvée');
      // Envoyer null pour déclencher une erreur dans Elm
      app.ports.routeLoadedFromLocalStorage.send(null);
    }
  } catch (error) {
    console.error('❌ Erreur lors du chargement:', error);
    app.ports.routeLoadedFromLocalStorage.send(null);
  }
});

// Center map on a location (geocoding result)
app.ports.centerMapOn.subscribe(({ lat, lon }) => {
  console.log('[Elm→JS] centerMapOn', { lat, lon });
  MapLibreMap.flyToBbox(lat, lon);
});

// ============================================================
// PORTS IN : JavaScript → Elm
// ============================================================

// Écouter les clics sur la carte
window.addEventListener('map-click', (event) => {
  console.log('[JS→Elm] map-click', event.detail);
  app.ports.mapClickReceived.send({
    lat: event.detail.lat,
    lon: event.detail.lon
  });
});

// Écouter le déplacement d'un waypoint (drag & drop)
window.addEventListener('waypoint-dragged', (event) => {
  console.log('[JS→Elm] waypoint-dragged', event.detail);
  app.ports.waypointDragged.send({
    index: event.detail.index,
    lat: event.detail.lat,
    lon: event.detail.lon
  });
});

// Écouter la suppression d'un waypoint (bouton ×)
window.addEventListener('waypoint-deleted', (event) => {
  console.log('[JS→Elm] waypoint-deleted', event.detail);
  app.ports.waypointDeleted.send({
    index: event.detail.index
  });
});

// Download GPX file
app.ports.downloadGpx.subscribe(({ filename, content }) => {
  console.log('[Elm→JS] downloadGpx', filename);
  const blob = new Blob([content], { type: 'application/gpx+xml' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
});

// Copy text to clipboard
app.ports.copyToClipboard.subscribe((text) => {
  console.log('[Elm→JS] copyToClipboard');
  const fullUrl = window.location.origin + window.location.pathname + text;
  navigator.clipboard.writeText(fullUrl).then(() => {
    console.log('✅ Link copied to clipboard');
  }).catch(err => {
    console.error('Failed to copy:', err);
  });
});

// Geolocation
app.ports.requestGeolocation.subscribe(() => {
  console.log('[Elm→JS] requestGeolocation');
  if ('geolocation' in navigator) {
    navigator.geolocation.getCurrentPosition(
      (pos) => {
        app.ports.gotGeolocation.send({
          lat: pos.coords.latitude,
          lon: pos.coords.longitude
        });
      },
      (err) => {
        console.warn('Geolocation error:', err.message);
      }
    );
  }
});

// ============================================================
// UNDO / REDO keyboard shortcuts (Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y)
// ============================================================
document.addEventListener('keydown', (e) => {
  // Skip if focus is in an input/textarea
  const tag = document.activeElement?.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;

  if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key === 'z') {
    e.preventDefault();
    app.ports.undoRedoReceived.send({ action: 'undo' });
  } else if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === 'z') {
    e.preventDefault();
    app.ports.undoRedoReceived.send({ action: 'redo' });
  } else if ((e.ctrlKey || e.metaKey) && e.key === 'y') {
    e.preventDefault();
    app.ports.undoRedoReceived.send({ action: 'redo' });
  }
});

// ============================================================
// GPX IMPORT
// ============================================================
app.ports.triggerGpxImport.subscribe(() => {
  console.log('[Elm→JS] triggerGpxImport');
  const input = document.createElement('input');
  input.type = 'file';
  input.accept = '.gpx';
  input.onchange = (e) => {
    const file = e.target.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (ev) => {
      try {
        const parser = new DOMParser();
        const doc = parser.parseFromString(ev.target.result, 'text/xml');
        // Extract track points, route points, or waypoints
        let points = Array.from(doc.querySelectorAll('trkpt'));
        if (points.length === 0) points = Array.from(doc.querySelectorAll('rtept'));
        if (points.length === 0) points = Array.from(doc.querySelectorAll('wpt'));

        const coords = points.map(pt => ({
          lat: parseFloat(pt.getAttribute('lat')),
          lon: parseFloat(pt.getAttribute('lon'))
        })).filter(c => !isNaN(c.lat) && !isNaN(c.lon));

        if (coords.length === 0) {
          console.warn('[GPX] No valid points found in file');
          return;
        }

        // Sample to ~15 waypoints max (first + last + evenly spaced)
        const MAX_WAYPOINTS = 15;
        let sampled;
        if (coords.length <= MAX_WAYPOINTS) {
          sampled = coords;
        } else {
          sampled = [coords[0]];
          const step = (coords.length - 1) / (MAX_WAYPOINTS - 1);
          for (let i = 1; i < MAX_WAYPOINTS - 1; i++) {
            sampled.push(coords[Math.round(i * step)]);
          }
          sampled.push(coords[coords.length - 1]);
        }

        console.log(`[GPX] Imported ${coords.length} points, sampled to ${sampled.length}`);
        app.ports.gpxWaypointsReceived.send(sampled);
      } catch (err) {
        console.error('[GPX] Parse error:', err);
      }
    };
    reader.readAsText(file);
  };
  input.click();
});

// ============================================================
// ELEVATION HOVER MARKER on map
// ============================================================
app.ports.setElevationHoverMarker.subscribe((coord) => {
  MapLibreMap.setElevationHoverMarker(coord);
});

// ============================================================
// CLOSE LOOP → Elm
// ============================================================
window.addEventListener('close-loop-clicked', () => {
  console.log('[JS→Elm] close-loop-clicked');
  app.ports.closeLoopRequested.send(true);
});

// ============================================================
// MAP ROUTE HOVER → Elm
// ============================================================
window.addEventListener('route-hover', (event) => {
  app.ports.mapRouteHover.send({ index: event.detail.index });
});

// Parse URL hash for shared waypoints on page load
(function parseUrlWaypoints() {
  const hash = window.location.hash;
  if (hash && hash.startsWith('#w=')) {
    const waypointStr = hash.substring(3);
    const points = waypointStr.split(';').map(p => {
      const [lat, lon] = p.split(',').map(Number);
      return { lat, lon };
    }).filter(p => !isNaN(p.lat) && !isNaN(p.lon));

    if (points.length >= 2) {
      console.log('[URL] Restoring', points.length, 'waypoints from URL');
      // Wait for map initialization, then simulate clicks
      setTimeout(() => {
        points.forEach(p => {
          app.ports.mapClickReceived.send({ lat: p.lat, lon: p.lon });
        });
      }, 1000);
    }
  }
})();

// ============================================================
// ORIENTEERING GAME PORTS (minimal — most logic is in Elm)
// ============================================================

// Show/hide MapLibre map
app.ports.setMapVisible.subscribe((visible) => {
  const mapEl = document.getElementById('map');
  if (mapEl) {
    mapEl.style.display = visible ? '' : 'none';
    // When showing map for topo overlay, make it fullscreen and resize
    if (visible) {
      mapEl.style.position = 'fixed';
      mapEl.style.top = '0';
      mapEl.style.left = '0';
      mapEl.style.width = '100vw';
      mapEl.style.height = '100vh';
      mapEl.style.zIndex = '5';
      mapEl.style.maxWidth = 'none';
      mapEl.style.borderRadius = '0';
      mapEl.style.border = 'none';
      mapEl.style.margin = '0';
      setTimeout(() => {
        window.dispatchEvent(new Event('resize'));
      }, 150);
    } else {
      // Reset to default styles
      mapEl.style.cssText = 'display: none;';
    }
  }
});

// Game scroll wheel → Elm
document.addEventListener('wheel', (e) => {
  app.ports.gameWheelReceived.send(e.deltaY);
}, { passive: true });

console.log('✅ Elm application initialized with MapLibre ports');
