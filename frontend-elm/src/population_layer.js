/**
 * Couche « Population » : carreaux INSEE Filosofi 2021 de 200 m, colorés par
 * densité. Un lieu sans carreau n'a aucun habitant et reste transparent : ce
 * sont justement les coins que cherche un chemin noir.
 * Les tuiles viennent de scripts/build_population_pmtiles.sh.
 */

export const POPULATION_SOURCE_ID = 'population';
/** Nom de la couche dans les tuiles, fixé par `-nln` dans le script. */
const SOURCE_LAYER = 'population';
const POPULATION_FILE = 'population.pmtiles';
/** En dessous, le script ne produit pas de tuiles (trop de carreaux). */
export const POPULATION_MIN_ZOOM = 10;
const POPULATION_MAX_ZOOM = 12;
/** Un carreau de 200 m × 200 m. */
const CELL_AREA_KM2 = 0.04;

/** Plafond de chaque classe, en habitants au km². La dernière n'en a pas. */
const SPARSE_MAX_PER_KM2 = 25;
const LOW_MAX_PER_KM2 = 250;
const MEDIUM_MAX_PER_KM2 = 1000;

/** Premier nombre entier d'habitants par carreau au-dessus d'un plafond de densité. */
function firstInhabitantsAbove(maxPerKm2) {
  return Math.floor(maxPerKm2 * CELL_AREA_KM2) + 1;
}

/** colors : { sparse, low, medium, dense } */
export function populationFillColor(colors) {
  return [
    'step', ['get', 'ind'],
    colors.sparse,
    firstInhabitantsAbove(SPARSE_MAX_PER_KM2), colors.low,
    firstInhabitantsAbove(LOW_MAX_PER_KM2), colors.medium,
    firstInhabitantsAbove(MEDIUM_MAX_PER_KM2), colors.dense
  ];
}

export function populationLayer({ id, colors, opacity }) {
  return {
    id,
    type: 'fill',
    source: POPULATION_SOURCE_ID,
    'source-layer': SOURCE_LAYER,
    minzoom: POPULATION_MIN_ZOOM,
    paint: { 'fill-color': populationFillColor(colors), 'fill-opacity': opacity }
  };
}

/** Le fichier est servi à côté de la page ; le protocole pmtiles le lit par plages. */
export function populationSource(origin) {
  return {
    type: 'vector',
    url: `pmtiles://${origin}/${POPULATION_FILE}`,
    minzoom: POPULATION_MIN_ZOOM,
    maxzoom: POPULATION_MAX_ZOOM,
    attribution: '&copy; <a href="https://www.insee.fr/fr/statistiques/8735162">Insee, Filosofi 2021</a>'
  };
}
