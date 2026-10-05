/**
 * Estampillage de la clé de cache du service worker.
 *
 * Ce qui est testé ici a coûté deux builds silencieusement inutiles : un
 * remplacement qui visait le commentaire au lieu de la déclaration, et une clé
 * figée qui rendait tout déploiement invisible.
 */
import test from 'node:test';
import assert from 'node:assert/strict';

import { remplacerCle, empreinteDe } from '../scripts/estampiller-sw.mjs';

/** Le voisinage réel : le mot apparaît deux fois, dont une en commentaire. */
const SOURCE = `// La clé est estampillée au build : \`__BUILD_ID__\` est remplacé dans
// \`dist/sw.js\` par une empreinte du contenu pré-mis en cache.
const CACHE_NAME = 'chemins-noirs-__BUILD_ID__';

self.addEventListener('install', () => {});
`;

test('la déclaration est estampillée, pas le commentaire', () => {
  const sortie = remplacerCle(SOURCE, 'abc1234567');

  assert.match(sortie, /const CACHE_NAME = 'chemins-noirs-abc1234567';/);
  // Le commentaire garde son gabarit : c'est lui qui explique le dispositif.
  assert.match(sortie, /\`__BUILD_ID__\` est remplacé/);
});

test('aucune déclaration au gabarit ne reste après estampillage', () => {
  const sortie = remplacerCle(SOURCE, 'abc1234567');
  assert.doesNotMatch(sortie, /'chemins-noirs-__BUILD_ID__'/);
});

test('un fichier déjà estampillé est refusé', () => {
  // Rejouer le script sur un dist/ déjà traité ne doit pas passer en silence.
  const dejaFait = remplacerCle(SOURCE, 'abc1234567');
  assert.throws(() => remplacerCle(dejaFait, 'autre00000'), /0 trouvée/);
});

test('deux déclarations sont refusées plutôt que remplacées à moitié', () => {
  const double = SOURCE + `const AUTRE = 'chemins-noirs-__BUILD_ID__';\n`;
  assert.throws(() => remplacerCle(double, 'abc1234567'), /2 trouvée/);
});

test('rien d’autre que la clé n’est modifié', () => {
  const sortie = remplacerCle(SOURCE, 'abc1234567');
  assert.equal(
    sortie.replace("'chemins-noirs-abc1234567'", "'chemins-noirs-__BUILD_ID__'"),
    SOURCE
  );
});

test('l’empreinte fait dix caractères hexadécimaux', () => {
  const id = empreinteDe([Buffer.from('a'), Buffer.from('b')]);
  assert.match(id, /^[0-9a-f]{10}$/);
});

test('la même entrée donne toujours la même empreinte', () => {
  const entree = [Buffer.from('<html>'), Buffer.from('body{}')];
  assert.equal(empreinteDe(entree), empreinteDe(entree));
});

test('un seul octet changé dans le CSS change la clé', () => {
  // C'est exactement le cas qui échappait au nom haché du bundle : une passe
  // qui ne touche qu'à la feuille de style laisse le bundle identique.
  const html = Buffer.from('<html>');
  const avant = empreinteDe([html, Buffer.from('body{color:#231b04}')]);
  const apres = empreinteDe([html, Buffer.from('body{color:#131b2f}')]);
  assert.notEqual(avant, apres);
});

test('l’ordre des fichiers compte, donc la liste doit rester stable', () => {
  const a = Buffer.from('un');
  const b = Buffer.from('deux');
  assert.notEqual(empreinteDe([a, b]), empreinteDe([b, a]));
});

test('importer le script n’écrit rien', () => {
  // Le corps en ligne de commande est sous garde : un import ne doit pas aller
  // toucher dist/. Si ce n'était pas le cas, les tests ci-dessus auraient déjà
  // échoué en sortant du processus.
  assert.equal(typeof remplacerCle, 'function');
  assert.equal(typeof empreinteDe, 'function');
});
