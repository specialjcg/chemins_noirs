# Chemins Noirs

Application web de planification d'itinéraires privilégiant les routes secondaires et chemins peu fréquentés.

## 🎯 Objectif

Générer des itinéraires évitant les axes principaux et favorisant :
- Les routes départementales et communales
- Les chemins forestiers et agricoles
- Les zones à faible densité de population
- Le relief et les paysages naturels

## ✨ Fonctionnalités

### Planification d'itinéraires
- **Point à point** : tracé simple entre deux points
- **Multi-points** : itinéraire passant par plusieurs waypoints
- **Boucles** : génération automatique de circuits fermés avec distance cible

### Visualisation
- **Carte 2D/3D** : basculement entre vue plane et relief 3D
- **Profil d'élévation** : visualisation du dénivelé avec données locales (DEM)
- **Vues satellite/standard** : fond de carte configurable
- **Animation drone** : survol 3D du parcours

### Gestion des tracés
- **Sauvegarde PostgreSQL** : persistance des itinéraires avec métadonnées
- **Re-traçage exact** : conservation des waypoints originaux pour recalcul identique
- **Export GPX** : téléchargement pour GPS/applications tierces
- **Favoris** : marquage des tracés préférés

## 🏗️ Architecture

### Backend (Rust)
- **Framework** : Axum (serveur HTTP asynchrone)
- **Routing** : Algorithme A* sur graphe OSM
- **Base de données** : PostgreSQL avec SQLx
- **Élévation** : DEM local (Arc/Info ASCII Grid) avec fallback Open-Meteo
- **Graph** : Génération partielle à la demande (bbox optimisée)

**Fichiers clés :**
- `backend/src/bin/backend_partial.rs` - API REST et handlers
- `backend/src/engine.rs` - Moteur de routage A*
- `backend/src/database.rs` - Couche PostgreSQL
- `backend/src/elevation.rs` - Profils d'élévation

### Frontend (Elm)
- **Architecture** : MVU (Model-View-Update) fonctionnelle pure
- **Carte** : MapLibre GL JS v5 avec terrain 3D natif
- **Build** : Vite (production optimisée ~300KB gzipped)
- **Communication** : Ports Elm ↔ JavaScript

**Fichiers clés :**
- `frontend-elm/src/Main.elm` - Logique MVU principale
- `frontend-elm/src/Types.elm` - Types immutables
- `frontend-elm/src/maplibre_map.js` - Intégration MapLibre

### Données
- **OSM** : OpenStreetMap (fichier PBF régional)
- **DEM** : Modèle numérique d'élévation local (SRTM/ASTER)
- **Tuiles** : MapTiler (satellite + relief)

## 🚀 Installation

### Prérequis

**Backend :**
```bash
# Rust (1.70+)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# PostgreSQL (15+)
sudo apt install postgresql postgresql-contrib

# GDAL (conversion DEM)
sudo apt install gdal-bin
```

**Frontend :**
```bash
# Node.js (18+) et npm
curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash -
sudo apt install nodejs

# Elm
npm install -g elm elm-format elm-test
```

### Configuration

**1. Base de données PostgreSQL**
```bash
cd backend
./setup_database.sh
```

Ce script crée :
- Base de données `chemins_noirs`
- Utilisateur `chemins_user`
- Table `saved_routes` avec migrations

**Ou manuellement :**
```bash
sudo -u postgres psql
CREATE DATABASE chemins_noirs;
CREATE USER chemins_user WITH PASSWORD 'votreMotDePasse';
GRANT ALL PRIVILEGES ON DATABASE chemins_noirs TO chemins_user;
\q
```

**2. Variables d'environnement**

Créer `backend/.env` :
```bash
DATABASE_URL=postgresql://chemins_user:votreMotDePasse@localhost/chemins_noirs
PBF_PATH=backend/data/rhone-alpes-251111.osm.pbf
CACHE_DIR=backend/data/cache
LOCAL_DEM_PATH=backend/data/dem/region.asc
```

**3. Données OSM**

Télécharger la région depuis [Geofabrik](https://download.geofabrik.de/) :
```bash
mkdir -p backend/data
cd backend/data
wget https://download.geofabrik.de/europe/france/rhone-alpes-latest.osm.pbf
```

**4. DEM (optionnel)**

Télécharger les tuiles SRTM et convertir :
```bash
mkdir -p backend/data/dem
cd backend/data/dem
# Télécharger SRTM .tif pour votre région
gdal_translate -of AAIGrid region.tif region.asc
```

## 🎮 Utilisation

### Lancement rapide

```bash
# À la racine du projet
./scripts/run_fullstack_elm.sh
```

L'application démarre sur :
- **Frontend** : http://localhost:3000
- **Backend** : http://localhost:8080

### Lancement manuel

**Backend :**
```bash
cd backend
DATABASE_URL="postgresql://..." cargo run --bin backend_partial
```

**Frontend :**
```bash
cd frontend-elm
npm install
npm run build
npm run preview -- --port 3000
```

### Utilisation de l'interface

**1. Tracer un itinéraire point à point**
- Cliquer sur la carte pour définir le départ (marqueur vert)
- Cliquer à nouveau pour l'arrivée (marqueur rouge)
- Ajuster les poids (population, routes pavées) si nécessaire
- Cliquer "Tracer l'itinéraire"

**2. Tracer un itinéraire multi-points**
- Basculer en mode "Multi-points"
- Cliquer sur la carte pour ajouter des waypoints
- Cocher "Boucle fermée" pour revenir au départ
- Cliquer "Tracer l'itinéraire"

**3. Générer une boucle**
- Basculer en mode "Boucle"
- Cliquer sur la carte pour le point de départ
- Définir la distance cible (km)
- Ajuster la tolérance et le nombre de candidats
- Cliquer "Générer boucles"
- Sélectionner un candidat dans la liste

**4. Sauvegarder un tracé**
- Après avoir tracé un itinéraire
- Cliquer sur "💾 Sauvegarder"
- Entrer un nom et description
- Le tracé est sauvegardé avec les waypoints originaux

**5. Charger un tracé**
- Cliquer sur "📂 Mes tracés"
- Sélectionner un tracé dans la liste
- Cliquer "Tracer l'itinéraire" pour recalculer avec les mêmes waypoints

## 🔧 Développement

### Structure du projet

```
chemins_noirs/
├── backend/
│   ├── src/
│   │   ├── bin/backend_partial.rs    # API REST
│   │   ├── engine.rs                 # Routage A*
│   │   ├── database.rs               # PostgreSQL
│   │   ├── elevation.rs              # Profils DEM
│   │   └── loops.rs                  # Génération boucles
│   ├── migrations/                   # SQL migrations
│   └── data/                         # OSM PBF + DEM + cache
│
├── frontend-elm/
│   ├── src/
│   │   ├── Main.elm                  # MVU principal
│   │   ├── Types.elm                 # Modèle de données
│   │   ├── Api.elm                   # HTTP client
│   │   ├── Decoders.elm              # JSON decoders
│   │   ├── Encoders.elm              # JSON encoders
│   │   ├── Ports.elm                 # Elm ↔ JS
│   │   ├── maplibre_map.js           # MapLibre GL
│   │   └── View/                     # Composants UI
│   └── tests/                        # Tests Elm
│
├── shared/
│   └── src/lib.rs                    # Types partagés Rust
│
└── scripts/
    └── run_fullstack_elm.sh          # Lancement automatique
```

### Tests

**Backend :**
```bash
cd backend
cargo test
cargo test --ignored  # Tests d'intégration avec DB
```

**Frontend :**
```bash
cd frontend-elm
elm-test
```

### Performance

**Optimisations bbox :**
- Margin réduite à 1km (au lieu de 5km)
- Réduction de 60-80% du temps de génération de graphe
- Cache des graphes partiels pour réutilisation

**Optimisations Elm :**
- Build production sans debugger
- Bundle optimisé ~300KB gzipped
- Lazy loading du DEM

## 📊 API REST

### Routes

**Routage :**
- `POST /api/route` - Point à point
- `POST /api/route/multi` - Multi-points
- `POST /api/loops` - Boucles

**Routes sauvegardées :**
- `GET /api/routes` - Liste
- `GET /api/routes/:id` - Détails
- `POST /api/routes` - Sauvegarder
- `DELETE /api/routes/:id` - Supprimer
- `POST /api/routes/:id/favorite` - Marquer favori

### Exemples

**Tracer un itinéraire :**
```bash
curl -X POST http://localhost:8080/api/route \
  -H "Content-Type: application/json" \
  -d '{
    "start": {"lat": 45.9309, "lon": 4.5778},
    "end": {"lat": 45.9405, "lon": 4.5756},
    "w_pop": 1.0,
    "w_paved": 1.0
  }'
```

**Sauvegarder :**
```bash
curl -X POST http://localhost:8080/api/routes \
  -H "Content-Type: application/json" \
  -d '[
    {
      "name": "Circuit forêt",
      "description": "Boucle 10km",
      "tags": ["foret", "boucle"]
    },
    {
      "path": [...],
      "distance_km": 10.5,
      ...
    }
  ]'
```

## 🐛 Troubleshooting

### Le backend ne démarre pas

**Erreur : "DATABASE_URL not set"**
```bash
# Vérifier .env
cat backend/.env
# Exporter manuellement
export DATABASE_URL="postgresql://chemins_user:pass@localhost/chemins_noirs"
```

**Erreur : "Failed to connect to PostgreSQL"**
```bash
# Vérifier que PostgreSQL tourne
sudo systemctl status postgresql
# Redémarrer si nécessaire
sudo systemctl restart postgresql
```

### Le routage est lent

**Première requête lente (génération de graphe)**
- Normal : génération du graphe partiel à la demande
- Suivant : utilise le cache (data/cache/*.json)

**Toujours lent**
- Vérifier la taille du PBF (région entière vs extrait)
- Vérifier les margins bbox (1km recommandé)

### Pas de profil d'élévation

**DEM local non trouvé**
```bash
# Vérifier le chemin
ls -lh backend/data/dem/region.asc
# Exporter la variable
export LOCAL_DEM_PATH="backend/data/dem/region.asc"
```

**Fallback Open-Meteo**
- Fonctionne automatiquement si DEM local absent
- Limite : 1000 points par requête

## 📝 Licence

Projet personnel - tous droits réservés.

## 🙏 Crédits

- **Données** : © OpenStreetMap contributors
- **DEM** : SRTM/ASTER GDEM
- **Tuiles** : MapTiler
- **Frameworks** : Rust, Elm, MapLibre GL JS
