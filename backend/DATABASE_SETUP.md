# 🗄️ Configuration PostgreSQL - Chemins Noirs

Guide pour configurer la base de données PostgreSQL pour sauvegarder les routes.

## Installation PostgreSQL

### Ubuntu/Debian
```bash
sudo apt update
sudo apt install postgresql postgresql-contrib
sudo systemctl start postgresql
sudo systemctl enable postgresql
```

### macOS
```bash
brew install postgresql@16
brew services start postgresql@16
```

## Configuration

### 1. Créer la base de données

```bash
# Se connecter en tant que postgres
sudo -u postgres psql

# Dans psql:
CREATE DATABASE chemins_noirs;
CREATE USER chemins_user WITH PASSWORD 'votre_mot_de_passe_securise';
GRANT ALL PRIVILEGES ON DATABASE chemins_noirs TO chemins_user;
\q
```

### 2. Variable d'environnement

Créer un fichier `.env` à la racine du projet :

```bash
# backend/.env
DATABASE_URL=postgresql://chemins_user:votre_mot_de_passe_securise@localhost/chemins_noirs
```

Ou exporter directement :

```bash
export DATABASE_URL="postgresql://chemins_user:votre_mot_de_passe_securise@localhost/chemins_noirs"
```

### 3. Exécuter les migrations

Les migrations sont exécutées automatiquement au démarrage du backend :

```bash
cd backend
cargo run --bin backend_partial
```

Ou manuellement avec sqlx-cli :

```bash
cargo install sqlx-cli --no-default-features --features postgres
cd backend
sqlx migrate run
```

## Schéma de la base de données

```sql
CREATE TABLE saved_routes (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    distance_km REAL NOT NULL,
    total_ascent_m REAL,
    total_descent_m REAL,
    route_data JSONB NOT NULL,
    gpx_data TEXT,
    is_favorite BOOLEAN DEFAULT FALSE,
    tags TEXT[] DEFAULT '{}'
);
```

## API Endpoints

Une fois configuré, les endpoints suivants seront disponibles :

### Sauvegarder une route
```http
POST /api/routes
Content-Type: application/json

{
  "name": "Tour du Mont Blanc",
  "description": "Belle randonnée alpine",
  "route": { ... },  // RouteResponse complet
  "tags": ["hiking", "alpine"]
}
```

### Lister toutes les routes
```http
GET /api/routes
```

### Obtenir une route spécifique
```http
GET /api/routes/:id
```

### Supprimer une route
```http
DELETE /api/routes/:id
```

### Basculer favori
```http
POST /api/routes/:id/favorite
```

## Vérification

Tester la connexion :

```bash
# Vérifier que PostgreSQL tourne
sudo systemctl status postgresql

# Tester la connexion
psql -U chemins_user -d chemins_noirs -h localhost
```

Dans psql :
```sql
-- Voir les routes sauvegardées
SELECT id, name, distance_km, created_at FROM saved_routes;

-- Compter les routes
SELECT COUNT(*) FROM saved_routes;
```

## Troubleshooting

### Erreur : `connection refused`
```bash
# Vérifier que PostgreSQL tourne
sudo systemctl status postgresql
sudo systemctl start postgresql
```

### Erreur : `FATAL: password authentication failed`
```bash
# Réinitialiser le mot de passe
sudo -u postgres psql
ALTER USER chemins_user WITH PASSWORD 'nouveau_mot_de_passe';
```

### Erreur : `DATABASE_URL not set`
```bash
# Vérifier la variable d'environnement
echo $DATABASE_URL

# Ou ajouter dans backend/.env
DATABASE_URL=postgresql://chemins_user:password@localhost/chemins_noirs
```

## Migration depuis localStorage

Les données actuellement dans localStorage du navigateur peuvent être sauvegardées dans PostgreSQL via l'interface une fois le backend configuré.

## Performance

- **Index automatiques** sur created_at, name, tags
- **Pool de connexions** : 5 connexions par défaut
- **JSONB** pour route_data : queries rapides sur les métadonnées

## Sécurité

⚠️ **Important** :
- Ne jamais commit le fichier `.env` avec les vrais mots de passe
- Utiliser des mots de passe forts en production
- Configurer SSL/TLS pour les connexions distantes
