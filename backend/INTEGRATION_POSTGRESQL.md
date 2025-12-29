# 🔧 Intégration PostgreSQL - Modifications nécessaires

## Modifications à apporter dans `backend_partial.rs`

### 1. Ajouter les imports en haut du fichier

```rust
use backend::database::Database;
use backend::saved_routes_handlers;
```

### 2. Modifier la fonction `main()` pour initialiser la base de données

Ajouter après la ligne `let config = Arc::new(PartialGraphConfig { ...});` (ligne ~361) :

```rust
// Initialize PostgreSQL database
let db = match Database::new().await {
    Ok(db) => {
        tracing::info!("PostgreSQL connected successfully");

        // Run migrations
        if let Err(e) = db.migrate().await {
            tracing::error!("Failed to run migrations: {}", e);
            panic!("Database migration failed");
        }

        Arc::new(db)
    }
    Err(e) => {
        tracing::warn!("PostgreSQL not available: {}. Routes won't be saved to database.", e);
        tracing::warn!("Set DATABASE_URL environment variable to enable database features.");
        // Option: continuer sans DB ou panic
        panic!("Database required");
    }
};
```

### 3. Remplacer les routes /api/routes dans le Router (lignes ~378-379)

**REMPLACER:**
```rust
.route("/api/routes/save", axum::routing::post(save_route_handler))
.route("/api/routes/load", axum::routing::get(load_route_handler))
```

**PAR:**
```rust
// Saved routes endpoints (PostgreSQL)
.route("/api/routes", axum::routing::get(saved_routes_handlers::list_routes))
.route("/api/routes", axum::routing::post(saved_routes_handlers::save_route))
.route("/api/routes/:id", axum::routing::get(saved_routes_handlers::get_route))
.route("/api/routes/:id", axum::routing::delete(saved_routes_handlers::delete_route))
.route("/api/routes/:id/favorite", axum::routing::post(saved_routes_handlers::toggle_favorite))
.with_state(db.clone())
```

### 4. Supprimer les anciens handlers (lignes ~19-75)

Supprimer complètement :
- `async fn save_route_handler(...)`
- `async fn load_route_handler(...)`

Ils sont remplacés par les handlers PostgreSQL dans `saved_routes_handlers.rs`.

### 5. Mettre à jour les logs de démarrage (lignes ~391-392)

**REMPLACER:**
```rust
tracing::info!("  POST /api/routes/save - Save route to disk");
tracing::info!("  GET /api/routes/load - Load saved route from disk");
```

**PAR:**
```rust
tracing::info!("  POST /api/routes - Save route to PostgreSQL");
tracing::info!("  GET /api/routes - List all saved routes");
tracing::info!("  GET /api/routes/:id - Get specific route");
tracing::info!("  DELETE /api/routes/:id - Delete route");
tracing::info!("  POST /api/routes/:id/favorite - Toggle favorite");
```

## Exemple complet de la section Router

```rust
let app = axum::Router::new()
    .route(
        "/api/graph/partial",
        axum::routing::post(backend::partial_graph::partial_graph_handler),
    )
    .route("/api/loops", axum::routing::post(loop_route_handler))
    .route("/api/route", axum::routing::post(route_handler))
    .route("/api/route/multi", axum::routing::post(multi_route_handler))
    .route("/api/click_mode", axum::routing::get(click_mode_handler))
    .layer(cors.clone())
    .with_state(config)
    // Saved routes with PostgreSQL - separate state
    .route("/api/routes", axum::routing::get(saved_routes_handlers::list_routes))
    .route("/api/routes", axum::routing::post(saved_routes_handlers::save_route))
    .route("/api/routes/:id", axum::routing::get(saved_routes_handlers::get_route))
    .route("/api/routes/:id", axum::routing::delete(saved_routes_handlers::delete_route))
    .route("/api/routes/:id/favorite", axum::routing::post(saved_routes_handlers::toggle_favorite))
    .layer(cors)
    .with_state(db);
```

## Variables d'environnement requises

Créer un fichier `.env` dans `backend/` :

```bash
# PostgreSQL connection
DATABASE_URL=postgresql://chemins_user:your_password@localhost/chemins_noirs

# Existing variables
PBF_PATH=data/rhone-alpes-251111.osm.pbf
CACHE_DIR=data/cache
```

## Test de compilation

```bash
cd backend
cargo check
cargo build
```

## Test de démarrage

```bash
# Avec PostgreSQL configuré
cargo run --bin backend_partial

# Vérifier les logs
# ✓ "PostgreSQL connected successfully"
# ✓ "Database migrations completed"
# ✓ "Starting backend on http://0.0.0.0:8080"
```

## Prochaines étapes

1. ✅ Appliquer ces modifications à `backend_partial.rs`
2. ⏳ Configurer PostgreSQL (voir DATABASE_SETUP.md)
3. ⏳ Modifier le frontend Elm pour utiliser la nouvelle API
4. ⏳ Tester l'intégration complète
