# 🚀 Scripts - Chemins Noirs

Scripts de démarrage pour l'application complète.

## Scripts disponibles

### `run_fullstack.sh` - Frontend Seed (Rust/WASM)

Lance l'application avec le frontend **Seed** (Rust/WASM) :

```bash
./scripts/run_fullstack.sh
```

- **Backend** : http://localhost:8080
- **Frontend** : http://localhost:8081 (Seed/WASM)
- **Bundle size** : ~300 KB
- **Hot reload** : ❌

### `run_fullstack_elm.sh` - Frontend Elm ⭐ RECOMMANDÉ

Lance l'application avec le frontend **Elm** (nouveau) :

```bash
./scripts/run_fullstack_elm.sh
```

- **Backend** : http://localhost:8080
- **Frontend** : http://localhost:3000 (Elm)
- **Bundle size** : ~30-50 KB (10x plus léger !)
- **Hot reload** : ✅ (modification instantanée)
- **Debugging** : Time-travel debugger intégré

## Comparaison

| Feature | Seed (run_fullstack.sh) | Elm (run_fullstack_elm.sh) |
|---------|-------------------------|----------------------------|
| **Bundle size** | ~300 KB | ~30-50 KB ⭐ |
| **Compile time** | 10-30s | 1-2s ⭐ |
| **Hot reload** | ❌ | ✅ ⭐ |
| **Debugging** | Console logs | Time-travel ⭐ |
| **Runtime errors** | Possibles | Zero garanti ⭐ |
| **Architecture** | MVU (Rust) | MVU (Elm) ⭐ |

## Utilisation

### Démarrage rapide

```bash
# 1. Installer les dépendances (première fois)
cd frontend-elm
npm install

# 2. Lancer l'application
cd ..
./scripts/run_fullstack_elm.sh
```

### Arrêt

```bash
# Ctrl+C dans le terminal
# Ou :
pkill -f "run_fullstack"
```

### Logs

Les logs des deux processus (backend + frontend) s'affichent dans le même terminal.

## Dépendances requises

### Backend (Rust)

- Rust 1.70+
- pkg-config
- cmake
- PostgreSQL 14+
- libpq-dev

```bash
sudo apt install pkg-config cmake postgresql postgresql-contrib libpq-dev build-essential
```

**Configuration PostgreSQL** (requis pour sauvegarder les routes):

```bash
cd backend
./setup_database.sh
```

Le script va créer automatiquement:
- La base de données `chemins_noirs`
- L'utilisateur `chemins_user`
- Les tables nécessaires
- Le fichier `.env` avec DATABASE_URL

### Frontend Elm

- Node.js 18+
- npm
- Elm 0.19.1

```bash
npm install -g elm elm-format elm-test
```

### Frontend Seed (ancien)

- Rust 1.70+
- wasm-pack
- Python 3 (pour serveur HTTP)

```bash
cargo install wasm-pack
```

## Troubleshooting

### Port déjà utilisé

Si vous voyez "Port busy", le script tue automatiquement les processus occupant les ports.

Si ça persiste :

```bash
# Tuer manuellement
lsof -ti :3000 | xargs kill -9  # Frontend Elm
lsof -ti :8081 | xargs kill -9  # Frontend Seed
lsof -ti :8080 | xargs kill -9  # Backend
```

### Backend ne compile pas

Voir [TROUBLESHOOTING.md](../TROUBLESHOOTING.md)

### PostgreSQL non configuré

Le backend peut démarrer sans PostgreSQL mais les routes ne seront pas sauvegardées.

Pour configurer PostgreSQL:

```bash
cd backend
./setup_database.sh
```

Ou manuellement:

```bash
sudo -u postgres psql
CREATE DATABASE chemins_noirs;
CREATE USER chemins_user WITH PASSWORD 'votre_mot_de_passe';
GRANT ALL PRIVILEGES ON DATABASE chemins_noirs TO chemins_user;
\q

# Puis créer backend/.env:
DATABASE_URL=postgresql://chemins_user:votre_mot_de_passe@localhost/chemins_noirs
```

Voir [backend/DATABASE_SETUP.md](../backend/DATABASE_SETUP.md) pour plus de détails.

### Frontend Elm : erreur "elm command not found"

```bash
npm install -g elm elm-format elm-test
```

### Frontend Elm : erreur au démarrage

```bash
cd frontend-elm
rm -rf node_modules elm-stuff
npm install
```

## Développement

### Mode développement (recommandé)

```bash
./scripts/run_fullstack_elm.sh
```

Hot reload activé : modifiez `frontend-elm/src/Main.elm` → le navigateur se recharge automatiquement !

### Build production

```bash
# Frontend Elm
cd frontend-elm
npm run build
# → dist/

# Servir avec nginx, Caddy, etc.
```

## Variables d'environnement

```bash
# Changer le fichier PBF
GRAPH_PBF=/path/to/your.osm.pbf ./scripts/run_fullstack_elm.sh

# Changer le répertoire cache
CACHE_DIR=/tmp/cache ./scripts/run_fullstack_elm.sh

# Utiliser un DEM local
LOCAL_DEM_PATH=/path/to/dem.asc ./scripts/run_fullstack_elm.sh

# Configurer PostgreSQL (automatiquement chargé depuis backend/.env)
DATABASE_URL=postgresql://user:password@localhost/chemins_noirs ./scripts/run_fullstack_elm.sh
```

**Note**: Le script charge automatiquement `DATABASE_URL` depuis `backend/.env` si ce fichier existe.

## Documentation

- **Frontend Elm** : [frontend-elm/README.md](../frontend-elm/README.md)
- **Migration Seed → Elm** : [MIGRATION_COMPLETE.md](../MIGRATION_COMPLETE.md)
- **Architecture MVU** : [MVU_COMPARISON.md](../MVU_COMPARISON.md)
- **Quickstart Elm** : [frontend-elm/QUICKSTART.md](../frontend-elm/QUICKSTART.md)

---

**Recommandation** : Utilisez `run_fullstack_elm.sh` pour bénéficier de tous les avantages d'Elm ! 🎯
