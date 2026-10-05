/**
 * Classement des couches du Plan IGN vectoriel pour le fond « Chemin noir ».
 *
 * Module séparé parce qu'il est pur : aucune dépendance à MapLibre, au DOM ni
 * au réseau. C'est ce qui le rend testable sous Node, et c'est là que vivent
 * les deux seuls pièges du dispositif — le suffixe des ouvrages d'art et la
 * mise à l'échelle des largeurs, qui se sont tous les deux trompés au premier
 * jet. Voir `test/chemin-noir.test.mjs`.
 */

/* ── Fond « Chemin noir » : la hiérarchie du Plan IGN, retournée ──────────────

   Toutes les cartes du monde dessinent l'autoroute en gras et le sentier en
   pointillé fin. Mesuré dans le style IGN, au zoom 17 : une route fait 16,8 à
   22 px de large, un sentier en fait 2. Un facteur dix en faveur de ce que
   cette application passe son temps à éviter.

   Le schéma vectoriel IGN sépare déjà `routier_route` de `routier_chemin`, et
   classe chaque objet par un attribut `symbo`. L'inversion ne demande donc
   aucune donnée nouvelle : elle échange le poids visuel de deux classes qui
   existent.

   Une trouvaille dans le schéma : `NON_REVETUE_CARRO` et
   `NON_REVETUE_CARRO_RESTREINT` désignent des routes **carrossables mais non
   revêtues**, rangées du côté des routes. Ce sont exactement des chemins
   noirs. On les promeut.

   Limite honnête : `CHEMIN` n'apparaît dans les tuiles qu'à partir du zoom 12,
   `SENTIER` du zoom 13. En dessous il n'y a rien à mettre en avant, et la
   carte retombe sur un réseau routier pâle. */

export const SYMBO_CHEMIN = new Set([
  'CHEMIN',
  'SENTIER',
  'VOIE_VERTE',
  'PISTE_CYCLABLE',
  'RUE_PIETONNE',
  'ESCALIER'
]);

export const SYMBO_ROUTE_NON_REVETUE = new Set([
  'NON_REVETUE_CARRO',
  'NON_REVETUE_CARRO_RESTREINT'
]);

/* Le schéma décline chaque classe en trois : au niveau du sol, en souterrain
   (`_SOU`) et en ouvrage d'art (`_SUP`). `NON_REVETUE_CARRO` a donc trois
   écritures, et ne reconnaître que la première renvoyait au goudron les routes
   non revêtues en tunnel ou sur pont. Trouvé en comptant les castes contre le
   style réel, pas en relisant. */
export function symboNu(valeur) {
  return String(valeur).replace(/_(SOU|SUP)$/, '');
}

/** Valeurs de `symbo` qu'un filtre de style IGN laisse passer. */
export function symboDuFiltre(filtre) {
  if (!Array.isArray(filtre)) return [];
  const [operateur, champ, ...valeurs] = filtre;
  if ((operateur === '==' || operateur === 'in') && champ === 'symbo') return valeurs;
  // Les filtres composés (all/any) enveloppent les précédents.
  if (operateur === 'all' || operateur === 'any') {
    return filtre.slice(1).flatMap(symboDuFiltre);
  }
  return [];
}

/**
 * Trois castes, et rien d'autre.
 *
 * `matiere`   ce qu'on cherche : chemins, sentiers, voies vertes, et les
 *             routes non revêtues promues depuis les routes.
 * `cicatrice` le goudron, réduit à un trait pâle.
 * `nulle`     tout le reste, laissé tel quel — relief, forêt, eau, toponymes.
 *             La forêt et l'eau restent lisibles parce que le moteur les
 *             récompense : les effacer reviendrait à cacher le raisonnement.
 */
export function casteIgn(meta) {
  const sl = meta.sourceLayer || '';
  if (sl.startsWith('routier_chemin')) return 'matiere';
  if (sl.startsWith('routier_route')) {
    const s = meta.symbo;
    if (s.length > 0 && s.every((v) => SYMBO_ROUTE_NON_REVETUE.has(symboNu(v)))) return 'matiere';
    return 'cicatrice';
  }
  if (sl === 'routier_surf' || sl.startsWith('routier_liaison')) return 'cicatrice';
  return 'nulle';
}

/** Met une valeur de largeur à l'échelle, qu'elle soit un nombre ou des paliers. */
export function largeurMiseALEchelle(valeur, facteur, plancher, plafond) {
  const borne = (x) => Math.min(plafond, Math.max(plancher, x * facteur));
  if (typeof valeur === 'number') return borne(valeur);
  if (valeur && Array.isArray(valeur.stops)) {
    return { ...valeur, stops: valeur.stops.map(([z, w]) => [z, borne(w)]) };
  }
  // Expression : on multiplie sans borner, faute de pouvoir lire la valeur.
  if (Array.isArray(valeur)) return ['*', valeur, facteur];
  return undefined;
}
