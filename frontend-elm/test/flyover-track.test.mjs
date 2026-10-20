/**
 * Tracé et caméra du survol : la géométrie qui place la caméra et les fins
 * d'étape. Tests de caractérisation, écrits avant de sortir ces fonctions de
 * flyover.js : ils figent le comportement de la version qui marche.
 */
import test from 'node:test';
import assert from 'node:assert/strict';

import {
  haversine,
  bearing,
  destination,
  smoothAngle,
  approach,
  createTrack,
  withElevations,
  pointAt,
  elevationAt,
  ascentBetween,
  slopeAt,
  stopsFromStages,
  stopsFromWaypoints
} from '../src/flyover_track.js';
import {
  PRESETS,
  DISTANCE_MIN_M,
  DISTANCE_MAX_M,
  ELEVATION_MIN_DEG,
  ELEVATION_MAX_DEG,
  constrainCamera,
  lookAheadM,
  cameraPlacement,
  distanceToSlider,
  sliderToDistance
} from '../src/flyover_camera.js';

const METERS_PER_DEGREE = 111195;

function nearly(actual, expected, tolerance, message) {
  assert.ok(Math.abs(actual - expected) <= tolerance, `${message} : ${actual} au lieu de ${expected}`);
}

/** Ligne droite vers le nord le long du méridien 0, un point par km. */
function northLine(pointCount) {
  return Array.from({ length: pointCount }, (_, index) => ({ lat: index * 1000 / METERS_PER_DEGREE, lon: 0 }));
}

test('haversine mesure un degré de latitude', () => {
  nearly(haversine({ lat: 0, lon: 0 }, { lat: 1, lon: 0 }), METERS_PER_DEGREE, 1, 'un degré');
});

test('bearing donne 0 vers le nord et 90 vers l’est', () => {
  nearly(bearing({ lat: 0, lon: 0 }, { lat: 1, lon: 0 }), 0, 1e-9, 'nord');
  nearly(bearing({ lat: 0, lon: 0 }, { lat: 0, lon: 1 }), 90, 1e-9, 'est');
  nearly(bearing({ lat: 0, lon: 0 }, { lat: 0, lon: -1 }), 270, 1e-9, 'ouest reste positif');
});

test('destination parcourt la distance demandée dans le cap demandé', () => {
  const origin = { lat: 45, lon: 5 };

  const arrival = destination(origin, 60, 2500);

  nearly(haversine(origin, arrival), 2500, 0.01, 'distance');
  nearly(bearing(origin, arrival), 60, 0.01, 'cap');
});

test('smoothAngle tourne par le plus court chemin à travers le nord', () => {
  nearly(smoothAngle(350, 10, 0.5), 0, 1e-9, 'de 350 vers 10');
  nearly(smoothAngle(10, 350, 0.5), 0, 1e-9, 'de 10 vers 350');
  assert.equal(smoothAngle(null, 42, 0.1), 42);
});

test('approach rejoint la cible au facteur 1 et part de la cible sans valeur', () => {
  assert.equal(approach(0, 10, 0.25), 2.5);
  assert.equal(approach(0, 10, 1), 10);
  assert.equal(approach(null, 10, 0.25), 10);
});

test('createTrack cumule les distances', () => {
  const track = createTrack(northLine(4));

  nearly(track.totalM, 3000, 0.01, 'longueur');
  assert.equal(track.cumul.length, 4);
  assert.equal(track.elevations, null);
});

test('createTrack accepte une absence de tracé', () => {
  const track = createTrack(null);

  assert.deepEqual(track.points, []);
  assert.equal(track.totalM, 0);
});

test('pointAt interpole et borne aux extrémités', () => {
  const track = createTrack(northLine(3));

  const halfway = (track.points[0].lat + track.points[1].lat) / 2;
  nearly(pointAt(track, track.cumul[1] / 2).lat, halfway, 1e-12, 'milieu du premier segment');
  assert.deepEqual(pointAt(track, -10), track.points[0]);
  assert.deepEqual(pointAt(track, 1e9), track.points[2]);
});

test('withElevations refuse un profil qui ne colle pas au tracé', () => {
  const track = createTrack(northLine(3));

  assert.equal(withElevations(track, [1, 2]), track);
  assert.deepEqual(withElevations(track, [1, 2, 3]).elevations, [1, 2, 3]);
});

test('elevationAt lit le profil et saute un trou', () => {
  const track = withElevations(createTrack(northLine(3)), [null, 120, 130]);

  assert.equal(elevationAt(track, 200), 120);
  assert.equal(elevationAt(track, 1500), 120);
  assert.equal(elevationAt(createTrack(northLine(3)), 200), null);
});

test('ascentBetween ne compte que les montées', () => {
  const track = withElevations(createTrack(northLine(5)), [100, 150, 120, null, 200]);

  assert.equal(ascentBetween(track, 0, track.totalM), 130);
  assert.equal(ascentBetween(createTrack(northLine(5)), 0, 1000), null);
});

test('slopeAt donne la pente sur 200 m autour du point, en pour cent', () => {
  const track = withElevations(createTrack(northLine(3)), [100, 200, 300]);

  // L'altitude d'un tronçon est celle de son début : à 1 km, 100 m → 200 m sur 200 m.
  nearly(slopeAt(track, 1000), 50, 1e-6, 'pente');
  assert.equal(slopeAt(createTrack(northLine(3)), 1500), null);
});

test('stopsFromStages écarte départ et arrivée', () => {
  const track = createTrack(northLine(11));
  const stages = [
    { endKm: 0, title: 'départ', detail: null },
    { endKm: 4, title: 'Étape 1', detail: 'gîte' },
    { endKm: 9.98, title: 'trop près de l’arrivée', detail: null }
  ];

  const stops = stopsFromStages(stages, track);

  assert.deepEqual(stops, [{ distanceM: 4000, title: 'Étape 1', detail: 'gîte' }]);
});

test('stopsFromWaypoints numérote les points intermédiaires et les trie', () => {
  const track = createTrack(northLine(11));
  const [start, , , atThreeKm, , , , atSevenKm, , , finish] = track.points;

  const stops = stopsFromWaypoints([start, atSevenKm, atThreeKm, finish], track);

  assert.deepEqual(stops.map(stop => stop.title), ['Point 3', 'Point 2']);
  nearly(stops[0].distanceM, 3000, 0.01, 'premier arrêt');
});

test('constrainCamera borne distance et hauteur', () => {
  const camera = constrainCamera({ ...PRESETS[0], distanceM: 1, elevationDeg: 120 });

  assert.equal(camera.distanceM, DISTANCE_MIN_M);
  assert.equal(camera.elevationDeg, ELEVATION_MAX_DEG);
  assert.equal(constrainCamera({ ...camera, distanceM: 1e9, elevationDeg: 0 }).distanceM, DISTANCE_MAX_M);
  assert.equal(constrainCamera({ ...camera, elevationDeg: 0 }).elevationDeg, ELEVATION_MIN_DEG);
});

test('lookAheadM suit la distance de caméra dans ses bornes', () => {
  assert.equal(lookAheadM({ distanceM: 100 }), 500);
  assert.equal(lookAheadM({ distanceM: 1000 }), 1500);
  assert.equal(lookAheadM({ distanceM: 8000 }), 3000);
});

test('cameraPlacement se place derrière le point, à la hauteur voulue', () => {
  const target = { lat: 45, lon: 5 };
  const camera = { distanceM: 1000, elevationDeg: 30, yawDeg: 0 };

  const placement = cameraPlacement(target, 0, camera);

  nearly(placement.heightM, 500, 1e-6, 'hauteur');
  nearly(haversine(target, placement.position), 1000 * Math.cos(Math.PI / 6), 0.01, 'recul');
  nearly(bearing(target, placement.position), 180, 0.01, 'derrière un cap nord');
});

test('le curseur de distance fait l’aller-retour sur son échelle logarithmique', () => {
  assert.equal(distanceToSlider(DISTANCE_MIN_M), 0);
  assert.equal(distanceToSlider(DISTANCE_MAX_M), 1000);
  nearly(sliderToDistance(distanceToSlider(1500)), 1500, 10, 'aller-retour');
});
