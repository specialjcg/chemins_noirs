let mapInstance;
let routeLayer;
let startMarker;
let endMarker;
let clickHandlerSet = false;
let standardLayer;
let satelliteLayer;
let currentLayer;
let bboxRectangle;

function ensureMap() {
  if (mapInstance) {
    return;
  }

  mapInstance = L.map("map").setView([45.0, 5.0], 8);

  // Standard OpenStreetMap layer
  standardLayer = L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
    attribution:
      '&copy; <a href="https://www.openstreetmap.org/copyright">OSM</a>',
    maxZoom: 18,
  });

  // Satellite layer (using Esri World Imagery)
  satelliteLayer = L.tileLayer("https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}", {
    attribution:
      'Tiles &copy; Esri &mdash; Source: Esri, i-cubed, USDA, USGS, AEX, GeoEye, Getmapping, Aerogrid, IGN, IGP, UPR-EGP, and the GIS User Community',
    maxZoom: 18,
  });

  // Add standard layer by default
  standardLayer.addTo(mapInstance);
  currentLayer = standardLayer;

  routeLayer = L.polyline([], { color: "#4dab7b", weight: 4 }).addTo(
    mapInstance,
  );
}

export function initMap() {
  ensureMap();
  if (!clickHandlerSet) {
    mapInstance.on("click", (event) => {
      console.debug("[map] click", event.latlng);
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
  console.debug("[map] updateRoute", coords);
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
  console.debug("[map] updateSelectionMarkers", start, end);
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

export function toggleSatelliteView(enabled) {
  ensureMap();
  console.debug("[map] toggleSatelliteView", enabled);

  if (enabled && currentLayer !== satelliteLayer) {
    mapInstance.removeLayer(currentLayer);
    satelliteLayer.addTo(mapInstance);
    currentLayer = satelliteLayer;
  } else if (!enabled && currentLayer !== standardLayer) {
    mapInstance.removeLayer(currentLayer);
    standardLayer.addTo(mapInstance);
    currentLayer = standardLayer;
  }
}

export function updateBbox(bounds) {
  ensureMap();
  console.debug("[map] updateBbox", bounds);

  // Remove existing bbox if any
  if (bboxRectangle) {
    mapInstance.removeLayer(bboxRectangle);
    bboxRectangle = null;
  }

  // Add new bbox if bounds provided
  if (bounds && typeof bounds.min_lat === "number") {
    const latLngBounds = [
      [bounds.min_lat, bounds.min_lon],
      [bounds.max_lat, bounds.max_lon],
    ];
    bboxRectangle = L.rectangle(latLngBounds, {
      color: "#ff7800",
      weight: 2,
      fillOpacity: 0.1,
      dashArray: "5, 5",
    }).addTo(mapInstance);

    console.debug("[map] BBox rectangle added");
  }
}
// Updated dim. 23 nov. 2025 15:59:57 CET
