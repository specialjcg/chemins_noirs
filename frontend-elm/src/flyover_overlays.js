/**
 * Ce que le survol pose sur la carte en plus de la caméra : le tracé déjà
 * parcouru et la carte affichée à chaque fin d'étape.
 */

const PROGRESS_LAYER_ID = 'flyover-progress';
/** line-gradient refuse un arrêt à 0 : le tracé parcouru commence juste après. */
const MIN_PROGRESS_FRACTION = 1e-6;
const UNDONE_COLOR = 'rgba(0, 0, 0, 0)';

function progressGradient(fraction, doneColor) {
  const doneUntil = Math.min(Math.max(fraction, MIN_PROGRESS_FRACTION), 1);
  return ['step', ['line-progress'], doneColor, doneUntil, UNDONE_COLOR];
}

/**
 * Tracé parcouru jusqu'à `fraction`. Une source fixe (tout le tracé) et un
 * dégradé en marche d'escalier sur line-progress : remplacer la géométrie à
 * chaque rafraîchissement faisait recalculer la source par MapLibre, d'où une
 * image perdue.
 */
export function showProgress(map, { points, fraction, doneColor }) {
  if (!map.getLayer(PROGRESS_LAYER_ID)) {
    addProgressLayer(map, points, doneColor);
    return;
  }
  map.setLayoutProperty(PROGRESS_LAYER_ID, 'visibility', 'visible');
  map.setPaintProperty(PROGRESS_LAYER_ID, 'line-gradient', progressGradient(fraction, doneColor));
}

export function hideProgress(map) {
  if (map.getLayer(PROGRESS_LAYER_ID)) map.setLayoutProperty(PROGRESS_LAYER_ID, 'visibility', 'none');
}

/** Le tracé a changé : la source du tracé parcouru est à refaire. */
export function dropProgress(map) {
  if (map.getLayer(PROGRESS_LAYER_ID)) map.removeLayer(PROGRESS_LAYER_ID);
  if (map.getSource(PROGRESS_LAYER_ID)) map.removeSource(PROGRESS_LAYER_ID);
}

function addProgressLayer(map, points, doneColor) {
  if (!map.isStyleLoaded()) return;
  const line = {
    type: 'Feature',
    properties: {},
    geometry: { type: 'LineString', coordinates: points.map(point => [point.lon, point.lat]) }
  };
  const source = map.getSource(PROGRESS_LAYER_ID);
  if (source) source.setData(line);
  else map.addSource(PROGRESS_LAYER_ID, { type: 'geojson', data: line, lineMetrics: true });
  map.addLayer({
    id: PROGRESS_LAYER_ID,
    type: 'line',
    source: PROGRESS_LAYER_ID,
    layout: { 'line-cap': 'round', 'line-join': 'round' },
    paint: { 'line-width': 6, 'line-opacity': 0.85, 'line-gradient': progressGradient(0, doneColor) }
  });
}

/** Carte de fin d'étape : titre et chiffres. Rend l'élément, à retirer ensuite. */
export function createStageCard(container, { title, facts }) {
  const card = document.createElement('div');
  card.className = 'flyover-card';
  const heading = document.createElement('strong');
  heading.textContent = title;
  const details = document.createElement('span');
  details.textContent = facts;
  card.append(heading, details);
  container.append(card);
  return card;
}
