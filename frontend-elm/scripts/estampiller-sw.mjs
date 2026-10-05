#!/usr/bin/env node
/**
 * Estampille la clé de cache du service worker, après `vite build`.
 *
 * `public/sw.js` est copié tel quel par Vite — rien n'y est transformé. Sa clé
 * de cache était donc figée à `chemins-noirs-v1`, et comme la purge à
 * l'activation ne supprime que les caches dont la clé diffère de la clé
 * courante, elle ne purgeait jamais rien : un visiteur gardait indéfiniment la
 * version de sa première visite, quel que soit le nombre de déploiements.
 *
 * Ce script remplace `__BUILD_ID__` dans `dist/sw.js` par une empreinte des
 * fichiers réellement pré-mis en cache. Volontairement pas le nom haché du
 * bundle : une passe qui ne touche qu'au CSS ne le change pas, et serait
 * repassée à travers.
 *
 * Il tourne après `vite build` plutôt que dans un greffon : la copie du
 * répertoire `public/` a lieu après le dernier point d'entrée des greffons, et
 * écrasait le fichier estampillé. Mesuré, pas supposé.
 */
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const RACINE = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const DIST = resolve(RACINE, 'dist');
const PRECACHE = ['index.html', 'style.css', 'manifest.json'];

const sw = resolve(DIST, 'sw.js');
if (!existsSync(sw)) {
  console.error('[sw] dist/sw.js introuvable — lancer vite build d’abord');
  process.exit(1);
}

const empreinte = createHash('sha1');
for (const nom of PRECACHE) {
  const chemin = resolve(DIST, nom);
  if (!existsSync(chemin)) {
    console.error(`[sw] ${nom} absent de dist/ : empreinte incomplète`);
    process.exit(1);
  }
  empreinte.update(readFileSync(chemin));
}
const id = empreinte.digest('hex').slice(0, 10);

// On vise la constante, pas le gabarit : le mot apparaît aussi dans le
// commentaire qui l'explique, et un `replace` sur une chaîne ne remplace que
// la première occurrence — il estampillait le commentaire en laissant la clé
// intacte, tout en annonçant avoir réussi.
const GABARIT = /'chemins-noirs-__BUILD_ID__'/g;

const source = readFileSync(sw, 'utf-8');
const trouvees = source.match(GABARIT);
if (!trouvees || trouvees.length !== 1) {
  console.error(
    `[sw] attendu une déclaration 'chemins-noirs-__BUILD_ID__' dans dist/sw.js, ${trouvees ? trouvees.length : 0} trouvée(s)`
  );
  process.exit(1);
}

writeFileSync(sw, source.replace(GABARIT, `'chemins-noirs-${id}'`));
console.log(`[sw] clé de cache : chemins-noirs-${id}`);
