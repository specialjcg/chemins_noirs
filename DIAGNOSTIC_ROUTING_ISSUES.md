# Diagnostic des Problèmes de Routage

**Date**: 2026-01-11
**Problèmes identifiés**: Performance lente, chemins en ligne droite, échecs de routage

## Résumé des Problèmes

### 1. Performance très lente (3-15 minutes par requête)
**Cause**: Graphes en cache ÉNORMES (1.9GB, 13M nœuds) générés pour de très grandes zones
**Symptôme**: Backend prend >50 secondes même pour de simples requêtes à 2 points

### 2. Chemins en ligne droite au lieu de suivre les routes
**Cause**: Les arêtes du graphe n'ont pas de waypoints intermédiaires OSM
**Symptôme**: Traces avec très peu de points, apparaissant comme des lignes droites

### 3. Échecs de routage ("No path found")
**Cause**: Points de test en dehors de la région ou trop loin des nœuds du graphe
**Symptôme**: Erreur "No path found for segment 1 -> 2"

## Analyse Détaillée

### Problème 1: Cache de Graphes Trop Grand

**Fichiers trouvés dans `backend/data/cache/`**:
```
122742ab62410966.json  - 1.9GB (13M nœuds, 2.6M arêtes)
148f9506f254916f.json  - 1.9GB (13M nœuds, 2.6M arêtes)
```

**Pourquoi c'est un problème**:
- Ces graphes ont été générés pour des requêtes avec beaucoup de waypoints sur une grande zone
- Le backend charge ces graphes en mémoire (2.4GB RAM utilisés)
- Les nouvelles requêtes avec des bbox similaires réutilisent ces énormes graphes
- L'algorithme A* sur 13M nœuds est extrêmement lent

**Solution appliquée**:
- ✅ Supprimé les fichiers JSON en cache (les .zst compressés restent)
- Besoin de redémarrer le backend pour vider le cache mémoire

### Problème 2: Absence de Waypoints Intermédiaires

**Log de débogage**:
```
DEBUG backend::engine: expand_path_with_waypoints: route had 6 nodes,
added 0 waypoints from edges, 5 edges had no waypoints
```

**Analyse du code**:
1. Le graphe a deux types de nœuds:
   - **Nœuds d'intersection**: Fins/débuts de segments (ajoutés au graphe)
   - **Nœuds intermédiaires**: Points le long des routes (stockés comme waypoints)

2. Fonction `build_edge_with_waypoints()` (graph.rs:1247):
   ```rust
   let waypoints: Vec<Coordinate> = node_refs[1..node_refs.len() - 1]
       .iter()
       .filter_map(|&osm_id| {
           let graph_id = *osm_to_graph.get(&osm_id)?;
           Some(coords[graph_id as usize])
       })
       .collect();
   ```

3. **Problème potentiel**: Les nœuds intermédiaires ne sont pas dans `osm_to_graph`

**Causes possibles**:
- Les ways OSM dans cette région n'ont pas beaucoup de nœuds intermédiaires
- Le code `filter_pbf_to_memory()` ne collecte pas tous les nœuds des ways
- Bug dans le mapping osm_id → graph_id pour les nœuds intermédiaires

**À investiguer**:
- Vérifier que `filter_pbf_to_memory()` collecte TOUS les nœuds référencés par les ways
- Vérifier que `build_from_filtered_data()` ajoute TOUS les nœuds au node_state
- Ajouter des logs de débogage pour compter les waypoints par edge

### Problème 3: Coordonnées de Test Invalides

**Coordonnées testées**:
```
Waypoint 1: lat=45.93, lon=4.57
Waypoint 2: lat=45.94, lon=4.58
```

**Analyse**:
- Lyon est à `45.75°N, 4.85°E`
- Les coordonnées testées (`4.57-4.58°E`) sont ~28km à l'OUEST de Lyon
- Cela pourrait être dans une zone rurale avec peu de routes

**Limites de proximité**:
- `MAX_DISTANCE_KM = 20.0` (engine.rs:289)
- Si le waypoint est à >20km du nœud le plus proche dans le graphe, `closest_node()` retourne `None`

**Coordonnées suggérées pour les tests** (centre de Lyon):
```javascript
{
  "waypoints": [
    {"lat": 45.760, "lon": 4.835},  // Place Bellecour
    {"lat": 45.770, "lon": 4.825}   // Fourvière
  ]
}
```

## Solutions Recommandées

### Solution Immédiate (Déjà Appliquée)
1. ✅ Nettoyé les fichiers JSON en cache (gardé les .zst)
2. ⏳ Besoin de redémarrer le backend

### Solutions Court Terme
1. **Implémenter le système de tiles** pour génération rapide (<10s):
   ```bash
   # Le script mentionne:
   export TILES_DIR=backend/data/tiles
   # Mais le dossier n'existe pas (seulement tiles.old)
   ```

2. **Réduire la marge de bbox** pour les petites requêtes:
   ```rust
   // backend/src/bin/backend_partial.rs:187
   // Actuellement: 5km margin
   // Suggéré: Marge adaptative basée sur la distance entre waypoints
   let margin_deg = if (max_lat - min_lat) < 0.1 {
       1.0 / 111.0  // 1km pour petites zones
   } else {
       5.0 / 111.0  // 5km pour grandes zones
   };
   ```

3. **Ajouter une limite de taille de graphe**:
   ```rust
   if graph.nodes.len() > 1_000_000 {
       tracing::warn!("Graph too large ({} nodes), consider using tiles",
                      graph.nodes.len());
   }
   ```

### Solutions Long Terme
1. **Déboguer la génération de waypoints**:
   - Ajouter des logs dans `build_edge_with_waypoints()`
   - Vérifier que les nœuds intermédiaires sont bien mappés
   - Tester avec un PBF plus petit

2. **Optimiser le cache**:
   - Éviction LRU basée sur la taille (pas seulement le nombre d'entrées)
   - TTL (Time-To-Live) pour les graphes en cache
   - Compression automatique des graphes >100MB

3. **Améliorer les performances**:
   - Utiliser le système de tiles (génération <10s au lieu de 3-15min)
   - Implémenter A* bidirectionnel
   - Ajouter un index spatial pour la recherche du nœud le plus proche

## Tests de Validation

### Test 1: Requête simple dans Lyon
```bash
curl -X POST http://localhost:8080/api/route/multi \
  -H "Content-Type: application/json" \
  -d '{
    "waypoints": [
      {"lat": 45.760, "lon": 4.835},
      {"lat": 45.770, "lon": 4.825}
    ],
    "w_pop": 1.5,
    "w_paved": 4.0,
    "close_loop": false
  }'
```

**Résultat attendu**:
- Génération de graphe < 5s (première fois) ou < 1s (depuis cache)
- Route avec au moins 50+ waypoints (pas juste 2-3 points)
- Distance ~1.5-2km

### Test 2: Vérifier les waypoints
```bash
# Compter les waypoints dans la réponse
curl ... | jq '.path | length'
```

**Résultat attendu**: >50 waypoints pour ~1.5km (densité ~30 points/km)

### Test 3: Performance avec cache vide
```bash
time curl ...
```

**Résultat attendu**:
- Avec tiles: <10s
- Sans tiles mais bbox optimisé: <30s
- Actuellement: 3-15 minutes (inacceptable)

## Fichiers Modifiés/À Modifier

### Fichiers à Investiguer
1. `backend/src/graph.rs:611` - `filter_pbf_to_memory()` - Collection des nœuds
2. `backend/src/graph.rs:749` - `build_from_filtered_data()` - Mapping osm_to_graph
3. `backend/src/graph.rs:1247` - `build_edge_with_waypoints()` - Génération waypoints

### Fichiers à Modifier (Court Terme)
1. `backend/src/bin/backend_partial.rs:187` - Marge de bbox adaptative
2. `backend/src/graph.rs:52` - Taille de cache LRU (actuellement 20)

## Logs Utiles pour Débogage

Ajouter dans `build_edge_with_waypoints()`:
```rust
tracing::debug!(
    "Edge {}->{}: segment has {} node_refs, {} in osm_to_graph, {} waypoints generated",
    from_id, to_id, node_refs.len(),
    node_refs.iter().filter(|id| osm_to_graph.contains_key(id)).count(),
    waypoints.len()
);
```

Ajouter dans `build_from_filtered_data()`:
```rust
tracing::info!(
    "Node collection: {} total nodes, {} intersection nodes",
    node_state.nodes.len(),
    intersections.len()
);
```

## Prochaines Étapes

1. ✅ Cache nettoyé
2. ⏳ Redémarrer le backend
3. 🔄 Tester avec coordonnées Lyon valides
4. 🔍 Investiguer génération de waypoints si le problème persiste
5. 🚀 Implémenter le système de tiles pour performance
