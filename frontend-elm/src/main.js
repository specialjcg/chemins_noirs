/**
 * Glue code Elm ↔ MapLibre GL JS
 * Connecte l'application Elm avec la carte MapLibre via les Ports
 */

// ============================================================
// Console → backend log forwarding (dev debug)
// Renvoie chaque console.{log,warn,error,info,debug} vers POST /api/log
// pour qu'ils apparaissent dans le terminal et dans frontend_debug.log
// ============================================================
(function installConsoleForwarder() {
  const LEVELS = ['log', 'info', 'warn', 'error', 'debug'];
  const originals = {};
  // Petit buffer + flush async pour éviter de saturer le réseau et garder l'ordre
  const queue = [];
  let flushScheduled = false;

  function serializeArg(a) {
    if (a instanceof Error) return a.stack || a.message;
    if (typeof a === 'string') return a;
    try { return JSON.stringify(a); } catch { return String(a); }
  }

  function scheduleFlush() {
    if (flushScheduled) return;
    flushScheduled = true;
    setTimeout(() => {
      flushScheduled = false;
      const batch = queue.splice(0, queue.length);
      if (batch.length === 0) return;
      // POST chaque ligne (handler backend prend une string par requête)
      for (const msg of batch) {
        fetch('/api/log', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ msg }),
          keepalive: true,
        }).catch(() => {/* swallow — pas de récursion sinon boucle infinie */});
      }
    }, 50);
  }

  for (const level of LEVELS) {
    originals[level] = console[level].bind(console);
    console[level] = function(...args) {
      originals[level](...args);
      try {
        const line = `[${level.toUpperCase()}] ` + args.map(serializeArg).join(' ');
        queue.push(line);
        scheduleFlush();
      } catch { /* ignore */ }
    };
  }

  // Capture les erreurs JS non catchées
  window.addEventListener('error', (e) => {
    queue.push(`[UNCAUGHT] ${e.message} @ ${e.filename}:${e.lineno}:${e.colno}\n${e.error?.stack || ''}`);
    scheduleFlush();
  });
  window.addEventListener('unhandledrejection', (e) => {
    queue.push(`[UNHANDLED_REJECTION] ${serializeArg(e.reason)}`);
    scheduleFlush();
  });

  originals.log('[devlog] console forwarding → /api/log actif');
})();

import maplibregl from 'maplibre-gl';

// Import de l'application Elm compilée
import { Elm } from './Main.elm';

// Import des fonctions MapLibre depuis le fichier original
// Note: Ce fichier doit être copié depuis frontend/maplibre_map.js
import * as MapLibreMap from './maplibre_map.js';

// Renderer Three.js pour la Course d'Orientation

// Initialiser l'application Elm
const app = Elm.Main.init({
  node: document.getElementById('app')
});
window._elmApp = app;

// ============================================================
// PORTS OUT : Elm → JavaScript
// ============================================================

// ============================================================
// Transitions de vue sur le changement d'onglet
// ============================================================
//
// `document.startViewTransition()` photographie l'écran, exécute son rappel,
// photographie à nouveau, puis anime entre les deux. Elm ne repeint qu'au
// `requestAnimationFrame` suivant : si on se contentait d'appeler le port, le
// rappel se terminerait avant qu'Elm ait touché au DOM et la transition
// comparerait deux fois la même image.
//
// D'où la promesse : on envoie le message à Elm, puis on attend deux trames.
// La première laisse Elm appliquer son correctif, la seconde laisse le style
// se recalculer. C'est seulement là qu'on résout, et que le navigateur prend
// sa deuxième photo.
//
// Deux portes de sortie, dans cet ordre : pas d'API, ou refus explicite des
// animations. Dans les deux cas on applique directement — l'onglet change,
// sans transition. Rien ne doit pouvoir empêcher un onglet de s'ouvrir.
function avecTransitionDeVue(sens, appliquer) {
  const mouvementRefuse = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  if (typeof document.startViewTransition !== 'function' || mouvementRefuse) {
    appliquer();
    return;
  }

  document.documentElement.dataset.sens = sens;

  // Deux trames pour laisser Elm appliquer son correctif, mais jamais
  // d'attente infinie : `requestAnimationFrame` est gelé dans un document
  // caché, et la promesse ne se résoudrait alors plus jamais. Mesuré : sans ce
  // délai, un changement d'onglet déclenché dans une fenêtre d'arrière-plan
  // laisse la transition ouverte et l'onglet ne bascule pas du tout. Le premier
  // des deux qui arrive gagne.
  const deuxTrames = () => new Promise((resoudre) => {
    let rendu = false;
    const unefois = () => {
      if (!rendu) {
        rendu = true;
        resoudre();
      }
    };
    requestAnimationFrame(() => requestAnimationFrame(unefois));
    setTimeout(unefois, 100);
  });

  let transition;
  try {
    transition = document.startViewTransition(() => {
      appliquer();
      return deuxTrames();
    });
  } catch (erreur) {
    // Si l'ouverture échoue, l'onglet doit changer quand même.
    delete document.documentElement.dataset.sens;
    appliquer();
    return;
  }

  // `ready` rejette dans deux cas parfaitement normaux : l'onglet du navigateur
  // est en arrière-plan (« Document hidden »), ou une seconde transition
  // démarre avant la fin de la première et l'avorte. Mesuré ici : en fenêtre
  // d'arrière-plan, Chrome rejette `ready` mais tient `finished` et exécute
  // bien le rappel — le panneau change, seule l'animation saute. Sans ce
  // `catch`, chacun de ces cas remonterait en rejet non traité dans la console.
  transition.ready.catch(() => {});
  transition.finished
    .catch(() => {})
    .finally(() => {
      delete document.documentElement.dataset.sens;
    });
}

// ============================================================
// Largeur du panneau : du CSS vers le CSS
// ============================================================
//
// `.app-container` porte `resize: horizontal`, donc l'utilisateur fixe sa
// largeur à la souris, sans une ligne de JavaScript. Mais trois règles de la
// feuille de style dépendent de `--panel-width` : la position du bouton de
// repli, et les deux coins de commandes de MapLibre qu'on avait dégagés de
// sous le panneau. Elles resteraient calées sur 480 px.
//
// D'où ce seul rôle : recopier la largeur réelle dans la variable. Pas de
// boucle à craindre — tant que personne n'a tiré la poignée, la largeur *vient*
// de la variable, et réécrire la même valeur ne déclenche pas d'observation ;
// dès qu'on a tiré, un style en ligne l'emporte sur la déclaration et la
// variable ne sert plus qu'aux voisins.
(function suivreLargeurDuPanneau(essais) {
  const panneau = document.querySelector('.app-container');
  if (!panneau) {
    // Elm n'a pas encore posé son premier rendu.
    if (essais < 50) {
      setTimeout(() => suivreLargeurDuPanneau(essais + 1), 50);
    }
    return;
  }

  let derniere = 0;
  new ResizeObserver((entrees) => {
    const boite = entrees[0].borderBoxSize && entrees[0].borderBoxSize[0];
    const largeur = Math.round(boite ? boite.inlineSize : panneau.getBoundingClientRect().width);
    if (Math.abs(largeur - derniere) < 1) {
      return;
    }
    derniere = largeur;
    document.documentElement.style.setProperty('--panel-width', largeur + 'px');
  }).observe(panneau);
})(0);

app.ports.demanderTransitionOnglet.subscribe(({ vers, sens }) => {
  avecTransitionDeVue(sens, () => app.ports.transitionOngletPrete.send(vers));
});

// Initialiser la carte
app.ports.initMap.subscribe(() => {
  console.log('[Elm→JS] initMap');
  MapLibreMap.initMap();
  // Fullscreen + Boucles buttons are MapLibre IControl instances
  // created inside maplibre_map.js initMap() → addControl().
});

// Mettre à jour la route affichée
app.ports.updateRoute.subscribe((coords) => {
  console.log('[Elm→JS] updateRoute', coords.length, 'points');
  MapLibreMap.updateRoute(coords);
});

// Mettre à jour les surfaces (paved/unpaved) de la route
app.ports.setRouteSurfaces.subscribe((surfaces) => {
  MapLibreMap.setRouteSurfaces(surfaces);
});

// Tronçons tirés tout droit et obstacles traversés
app.ports.setDirectOverlay.subscribe((overlay) => {
  MapLibreMap.setDirectOverlay(overlay);
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
  MapLibreMap.centerMapOnCoord(lat, lon);
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

// Écouter les clics tombés sur le tracé : ils insèrent un point au lieu d'en
// ajouter un à la fin.
window.addEventListener('route-line-click', (event) => {
  console.log('[JS→Elm] route-line-click', event.detail);
  app.ports.routeLineClicked.send({
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
app.ports.setReferenceRoutes.subscribe((routes) => {
  MapLibreMap.setReferenceRoutes(routes);
});

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

console.log('✅ Elm application initialized with MapLibre ports');
