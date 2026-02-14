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

// Mettre à jour les marqueurs de sélection (départ/arrivée)
app.ports.updateSelectionMarkers.subscribe(({ start, end }) => {
  console.log('[Elm→JS] updateSelectionMarkers', { start, end });
  MapLibreMap.updateSelectionMarkers(start, end);
});

// Mettre à jour les marqueurs de waypoints (mode multi-point)
app.ports.updateWaypointMarkers.subscribe((waypoints) => {
  console.log('[Elm→JS] updateWaypointMarkers', waypoints.length, 'waypoints');
  MapLibreMap.updateWaypointMarkers(waypoints);
});

// Basculer vue satellite/standard (legacy)
app.ports.toggleSatelliteView.subscribe((enabled) => {
  console.log('[Elm→JS] toggleSatelliteView', enabled);
  MapLibreMap.toggleSatelliteView(enabled);
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

console.log('✅ Elm application initialized with MapLibre ports');
