let mapInstance;
let routeLayer;
let startMarker;
let endMarker;
let clickHandlerSet = false;

function ensureMap() {
  if (mapInstance) {
    return;
  }

  mapInstance = L.map("map").setView([45.0, 5.0], 8);
  L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
    attribution:
      '&copy; <a href="https://www.openstreetmap.org/copyright">OSM</a>',
    maxZoom: 18,
  }).addTo(mapInstance);
  routeLayer = L.polyline([], { color: "#4dab7b", weight: 4 }).addTo(
    mapInstance,
  );
}

export function initMap() {
  ensureMap();
  if (!clickHandlerSet) {
    mapInstance.on("click", (event) => {
      window.dispatchEvent(
        new CustomEvent("map-click", {
          detail: { lat: event.latlng.lat, lon: event.latlng.lng },
        }),
      );
    });
    clickHandlerSet = true;
  }
}

export function updateRoute(coords) {
  ensureMap();
  if (!Array.isArray(coords) || coords.length === 0) {
    routeLayer.setLatLngs([]);
    return;
  }
  const latlngs = coords.map((c) => [c.lat, c.lon]);
  routeLayer.setLatLngs(latlngs);
  const bounds = routeLayer.getBounds();
  if (bounds.isValid()) {
    mapInstance.fitBounds(bounds, { padding: [24, 24] });
  }
}

export function updateSelectionMarkers(start, end) {
  ensureMap();
  updateMarker("start", start);
  updateMarker("end", end);
}

function updateMarker(type, coord) {
  let markerRef = type === "start" ? startMarker : endMarker;
  if (coord && typeof coord.lat === "number" && typeof coord.lon === "number") {
    if (!markerRef) {
      markerRef = L.marker([coord.lat, coord.lon], {
        title: type === "start" ? "Départ" : "Arrivée",
      }).addTo(mapInstance);
      if (type === "start") startMarker = markerRef;
      else endMarker = markerRef;
    } else {
      markerRef.setLatLng([coord.lat, coord.lon]);
    }
  } else if (markerRef) {
    mapInstance.removeLayer(markerRef);
    if (type === "start") startMarker = null;
    else endMarker = null;
  }
}
