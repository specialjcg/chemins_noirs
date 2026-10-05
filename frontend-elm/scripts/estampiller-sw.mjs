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

/**
 * Remplace le gabarit de clé de cache par l'identifiant du build.
 *
 * On vise la déclaration, pas le mot : `__BUILD_ID__` apparaît aussi dans le
 * commentaire de `sw.js` qui l'explique, et `String.replace` avec une chaîne
 * ne remplace que la première occurrence. Le premier jet estampillait donc le
 * commentaire, laissait la clé intacte, et annonçait quand même sa réussite.
 * D'où l'expression régulière ancrée sur les guillemets, et le refus net si
 * elle ne trouve pas exactement une déclaration.
 *
 * @throws si le gabarit est absent ou présent plusieurs fois.
 */
export function remplacerCle(source, id) {
  const gabarit = /'chemins-noirs-__BUILD_ID__'/g;
  const trouvees = source.match(gabarit);
  if (!trouvees || trouvees.length !== 1) {
    throw new Error(
      `attendu une déclaration 'chemins-noirs-__BUILD_ID__', ${trouvees ? trouvees.length : 0} trouvée(s)`
    );
  }
  return source.replace(gabarit, `'chemins-noirs-${id}'`);
}

/**
 * Empreinte des fichiers pré-mis en cache.
 *
 * Volontairement pas le nom haché du bundle : une passe qui ne touche qu'au
 * CSS ne le change pas, et serait repassée à travers.
 */
export function empreinteDe(contenus) {
  const h = createHash('sha1');
  contenus.forEach((c) => h.update(c));
  return h.digest('hex').slice(0, 10);
}

const RACINE = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const DIST = resolve(RACINE, 'dist');
const PRECACHE = ['index.html', 'style.css', 'manifest.json'];

function principal() {
  const sw = resolve(DIST, 'sw.js');
  if (!existsSync(sw)) {
    console.error('[sw] dist/sw.js introuvable — lancer vite build d’abord');
    process.exit(1);
  }

  const contenus = [];
  for (const nom of PRECACHE) {
    const chemin = resolve(DIST, nom);
    if (!existsSync(chemin)) {
      console.error(`[sw] ${nom} absent de dist/ : empreinte incomplète`);
      process.exit(1);
    }
    contenus.push(readFileSync(chemin));
  }
  const id = empreinteDe(contenus);

  let estampille;
  try {
    estampille = remplacerCle(readFileSync(sw, 'utf-8'), id);
  } catch (erreur) {
    console.error('[sw] ' + erreur.message);
    process.exit(1);
  }

  writeFileSync(sw, estampille);
  console.log(`[sw] clé de cache : chemins-noirs-${id}`);
}

// Le corps ne s'exécute que lancé en ligne de commande : les tests importent
// ce fichier pour éprouver `remplacerCle` et `empreinteDe`, et n'ont aucune
// raison d'aller écrire dans dist/.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  principal();
}
