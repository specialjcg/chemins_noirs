let mapInstance;
let routeLayer;

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
