# Fix: Routing avec lignes droites → Routes exactes

## Problème identifié

**Symptôme**: La trace verte montre des lignes droites entre les points au lieu de suivre les routes.

**Cause**: Les tiles générées étaient invalides (427 MB chacune avec 13M nodes au lieu de ~1-5 MB avec 50-100K nodes), créant un graphe déconnecté.

## Solution appliquée

✅ Tiles invalides désactivées (renommées en `tiles.INVALID`)
✅ Retour au mode PBF standard (4 passes - fiable et précis)
✅ Cache DEM binaire activé (6 min → <5s après première utilisation)

## Performances attendues (mode PBF)

### Première requête (création cache DEM)
- Génération graph: ~2 minutes (PBF 4 passes)
- Chargement DEM: ~6 minutes (création cache binaire)
- **Total: ~8 minutes** (une seule fois)

### Deuxième requête (même zone)
- Génération graph: <2 secondes (cache disque)
- Chargement DEM: <5 secondes (cache binaire)
- **Total: <10 secondes** ✅

### Autres requêtes (nouvelles zones)
- Génération graph: ~2 minutes (nouvelle zone PBF)
- Chargement DEM: <5 secondes (cache binaire)
- **Total: ~2 minutes** ✅

## Test maintenant

```bash
cd /home/jcgouleau/IdeaProjects/RustProject/chemins_noirs

# Restart le backend (Ctrl+C dans le terminal actuel d'abord)
./scripts/run_fullstack_elm.sh
```

**Attendez les logs:**
```
ℹ️  No tiles directory - using PBF mode (~2min first request)
```

**Créez une route dans le frontend.**

**Vérifiez:**
- ✅ La trace suit exactement les routes (pas de lignes droites)
- ✅ Temps: ~8 min pour la première (cache DEM), ~2 min pour les suivantes
- ✅ Deuxième route dans la même zone: <10s

## Option 2: Régénérer les tiles correctement

**Si vous voulez <15 secondes dès la première requête:**

### Étape 1: Nettoyer les tiles invalides

```bash
rm -rf backend/data/tiles.INVALID
mkdir -p backend/data/tiles
```

### Étape 2: Régénérer les tiles (3-5 heures)

```bash
./scripts/generate_tiles.sh
```

**Ce qui va se passer:**
- Lecture PBF: ~2 min par tile
- ~100-150 tiles non-vides pour Rhône-Alpes
- Total: 3-5 heures
- Espace disque: ~500 MB - 2 GB

**Progression:**
```
[1/100] Generating tile TileId { x: 17, y: 254 }
  ✅ Saved: 52341 nodes, 54123 edges → tile_17_254.json.zst
[2/100] Generating tile TileId { x: 17, y: 255 }
  ✅ Saved: 48923 nodes, 51002 edges → tile_17_255.json.zst
...
```

**Vous pouvez interrompre (Ctrl+C) et reprendre - les tiles déjà créées sont skip.**

### Étape 3: Tester avec tiles

```bash
# Les tiles seront détectées automatiquement
./scripts/run_fullstack_elm.sh
```

**Attendez les logs:**
```
🚀 Tiles directory found: backend/data/tiles (FAST MODE enabled - <10s per route)
```

**Performance avec tiles:**
- Première requête: ~10 secondes (tiles + DEM cache)
- Requêtes suivantes: <15 secondes

## Résumé

| Mode | Première requête | Requêtes suivantes | Précision |
|------|------------------|-------------------|-----------|
| **PBF (actuel)** | ~8 min | ~2 min | ✅ Exacte |
| **Tiles (après regen)** | ~15 s | ~15 s | ✅ Exacte |

**Recommandation:**
1. **Testez d'abord** le mode PBF pour confirmer que le routing est correct
2. **Si le routing est bon**, lancez la régénération des tiles en arrière-plan
3. **Une fois les tiles générées**, vous aurez <15s pour toutes les requêtes

## Si le routing ne fonctionne toujours pas

Si même en mode PBF les routes tracent des lignes droites:
1. Vérifiez les logs backend pour des erreurs
2. Vérifiez que le graphe contient bien des nodes/edges:
   ```
   Engine created: 30814 nodes, 31945 edges
   ```
3. Partagez les logs pour diagnostic

---

**Statut actuel**: Mode PBF activé, tiles désactivées. Testez une route maintenant !
