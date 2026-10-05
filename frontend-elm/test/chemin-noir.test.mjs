/**
 * Classement des couches IGN pour le fond « Chemin noir ».
 *
 * Ces tests ne sont pas décoratifs : chacun des deux premiers blocs fige un
 * défaut qui s'est réellement produit, et qu'aucune relecture n'avait vu.
 */
import test from 'node:test';
import assert from 'node:assert/strict';

import {
  symboNu,
  symboDuFiltre,
  casteIgn,
  largeurMiseALEchelle
} from '../src/chemin_noir.js';

/** Égalité à l'arrondi près : 1.4 × 3.5 vaut 4.899999999999999 en binaire. */
function presque(reel, attendu, message) {
  assert.ok(
    Math.abs(reel - attendu) < 1e-9,
    `${message} : attendu ${attendu}, obtenu ${reel}`
  );
}

test('symboNu retire le suffixe des ouvrages d’art', () => {
  // Le schéma IGN décline chaque classe en trois : au niveau du sol, en
  // souterrain, en ouvrage supérieur. Ne reconnaître que la forme nue renvoyait
  // au goudron les routes non revêtues en tunnel ou sur pont.
  assert.equal(symboNu('NON_REVETUE_CARRO_SOU'), 'NON_REVETUE_CARRO');
  assert.equal(symboNu('NON_REVETUE_CARRO_SUP'), 'NON_REVETUE_CARRO');
  assert.equal(symboNu('NON_REVETUE_CARRO'), 'NON_REVETUE_CARRO');
});

test('symboNu ne coupe que le suffixe, jamais le milieu d’un nom', () => {
  assert.equal(symboNu('NON_REVETUE_CARRO_RESTREINT'), 'NON_REVETUE_CARRO_RESTREINT');
  assert.equal(symboNu('SOUS_BOIS'), 'SOUS_BOIS');
  assert.equal(symboNu('SUPERIEURE'), 'SUPERIEURE');
});

test('symboDuFiltre lit les deux écritures du style IGN', () => {
  assert.deepEqual(symboDuFiltre(['==', 'symbo', 'SENTIER']), ['SENTIER']);
  assert.deepEqual(symboDuFiltre(['in', 'symbo', 'PISTE_CYCLABLE', 'VOIE_VERTE']), [
    'PISTE_CYCLABLE',
    'VOIE_VERTE'
  ]);
});

test('symboDuFiltre descend dans les filtres composés', () => {
  const filtre = ['all', ['==', '$type', 'LineString'], ['==', 'symbo', 'CHEMIN']];
  assert.deepEqual(symboDuFiltre(filtre), ['CHEMIN']);
});

test('symboDuFiltre ignore les filtres qui ne portent pas sur symbo', () => {
  assert.deepEqual(symboDuFiltre(['==', 'nature', 'Sentier']), []);
  assert.deepEqual(symboDuFiltre(null), []);
  assert.deepEqual(symboDuFiltre(undefined), []);
});

test('les chemins sont de la matière', () => {
  for (const sl of ['routier_chemin', 'routier_chemin_sou', 'routier_chemin_sup']) {
    assert.equal(
      casteIgn({ sourceLayer: sl, symbo: ['SENTIER'], type: 'line' }),
      'matiere',
      sl
    );
  }
});

test('les routes non revêtues sont promues, tunnels et ponts compris', () => {
  const cas = [
    'NON_REVETUE_CARRO',
    'NON_REVETUE_CARRO_SOU',
    'NON_REVETUE_CARRO_SUP',
    'NON_REVETUE_CARRO_RESTREINT',
    'NON_REVETUE_CARRO_RESTREINT_SOU'
  ];
  for (const symbo of cas) {
    assert.equal(
      casteIgn({ sourceLayer: 'routier_route', symbo: [symbo], type: 'line' }),
      'matiere',
      symbo
    );
  }
});

test('le goudron reste une cicatrice', () => {
  const cas = ['LOCALE_2', 'REGIONALE_1', 'PRINCIPALE_3', 'BRET_AUTO_PEAGE_1', 'NON_CLASSEE'];
  for (const symbo of cas) {
    assert.equal(
      casteIgn({ sourceLayer: 'routier_route', symbo: [symbo], type: 'line' }),
      'cicatrice',
      symbo
    );
  }
  assert.equal(casteIgn({ sourceLayer: 'routier_surf', symbo: [], type: 'fill' }), 'cicatrice');
});

test('une couche mêlant revêtu et non revêtu reste du goudron', () => {
  // La promotion exige que *toutes* les valeurs du filtre soient non revêtues :
  // épaissir une couche qui contient aussi des routes goudronnées les
  // épaissirait avec.
  assert.equal(
    casteIgn({
      sourceLayer: 'routier_route',
      symbo: ['NON_REVETUE_CARRO', 'LOCALE_2'],
      type: 'line'
    }),
    'cicatrice'
  );
});

test('une couche de route sans filtre sur symbo reste du goudron', () => {
  assert.equal(casteIgn({ sourceLayer: 'routier_route', symbo: [], type: 'line' }), 'cicatrice');
});

test('le décor n’est pas touché', () => {
  for (const sl of ['ocs_vegetation_surf', 'hydro_reseau', 'oro_courbe', 'toponyme_bati_ponc', 'bati_surf']) {
    assert.equal(casteIgn({ sourceLayer: sl, symbo: [], type: 'fill' }), 'nulle', sl);
  }
  assert.equal(casteIgn({ sourceLayer: undefined, symbo: [], type: 'background' }), 'nulle');
});

test('la mise à l’échelle accepte un nombre et respecte ses bornes', () => {
  presque(largeurMiseALEchelle(2, 3.5, 1.6, 11), 7, 'nombre simple');
  assert.equal(largeurMiseALEchelle(0.1, 3.5, 1.6, 11), 1.6, 'plancher');
  assert.equal(largeurMiseALEchelle(100, 3.5, 1.6, 11), 11, 'plafond');
});

test('la mise à l’échelle traverse les paliers du style IGN', () => {
  // Les largeurs réelles d'un sentier, relevées dans PLAN.IGN/standard.json.
  const sentier = { stops: [[14, 1], [15, 1.2], [16, 1.4], [17, 2]] };
  const obtenu = largeurMiseALEchelle(sentier, 3.5, 1.6, 11);
  const attendu = [[14, 3.5], [15, 4.2], [16, 4.9], [17, 7]];
  assert.equal(obtenu.stops.length, attendu.length);
  obtenu.stops.forEach(([z, w], i) => {
    assert.equal(z, attendu[i][0], 'le zoom ne doit pas bouger');
    presque(w, attendu[i][1], `palier z${z}`);
  });
});

test('le renversement est effectif au zoom 17', () => {
  // Avant : la route fait onze fois la largeur du sentier.
  // Après : le sentier fait trois fois et demie celle de la route.
  const sentier = { stops: [[17, 2]] };
  const route = { stops: [[17, 22.5]] };
  const sApres = largeurMiseALEchelle(sentier, 3.5, 1.6, 11).stops[0][1];
  const rApres = largeurMiseALEchelle(route, 1 / 6, 0.5, 2).stops[0][1];
  assert.ok(22.5 / 2 > 11, 'la hiérarchie de départ favorise bien la route');
  presque(sApres, 7, 'sentier après');
  assert.equal(rApres, 2, 'route après, au plafond');
  assert.ok(sApres / rApres >= 3.5, `renversé : ${sApres} contre ${rApres}`);
});

test('une largeur en expression est multipliée sans être bornée', () => {
  // On ne sait pas lire la valeur d'une expression : on la multiplie, et on
  // assume de ne pas pouvoir la borner.
  const expr = ['interpolate', ['linear'], ['zoom'], 14, 1, 17, 2];
  assert.deepEqual(largeurMiseALEchelle(expr, 3.5, 1.6, 11), ['*', expr, 3.5]);
});

test('une largeur absente ne produit rien plutôt que NaN', () => {
  assert.equal(largeurMiseALEchelle(undefined, 3.5, 1.6, 11), undefined);
  assert.equal(largeurMiseALEchelle(null, 3.5, 1.6, 11), undefined);
});
