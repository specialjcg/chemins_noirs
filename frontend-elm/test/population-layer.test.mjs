/**
 * Couche « Population » : classes de densité des carreaux INSEE de 200 m.
 */
import test from 'node:test';
import assert from 'node:assert/strict';

import { populationFillColor, populationLayer, POPULATION_SOURCE_ID } from '../src/population_layer.js';

const COLORS = { sparse: 'pale', low: 'orange', medium: 'red', dense: 'darkred' };

/** Évalue une expression MapLibre `step` sur la propriété `ind`. */
function colorFor(inhabitants) {
  const [, , base, ...stops] = populationFillColor(COLORS);
  let color = base;
  for (let index = 0; index < stops.length; index += 2) {
    if (inhabitants >= stops[index]) color = stops[index + 1];
  }
  return color;
}

test('un habitant sur 4 ha reste sous 25 hab/km²', () => {
  assert.equal(colorFor(1), 'pale');
});

test('jusqu’à 250 hab/km², soit 10 habitants par carreau', () => {
  assert.equal(colorFor(2), 'orange');
  assert.equal(colorFor(10), 'orange');
});

test('jusqu’à 1000 hab/km², soit 40 habitants par carreau', () => {
  assert.equal(colorFor(11), 'red');
  assert.equal(colorFor(40), 'red');
});

test('au-delà de 1000 hab/km²', () => {
  assert.equal(colorFor(41), 'darkred');
});

test('la couche remplit les carreaux de la source population', () => {
  const layer = populationLayer({ id: 'population-cells', colors: COLORS, opacity: 0.5 });
  assert.equal(layer.type, 'fill');
  assert.equal(layer.source, POPULATION_SOURCE_ID);
  assert.equal(layer['source-layer'], 'population');
  assert.equal(layer.paint['fill-opacity'], 0.5);
});
