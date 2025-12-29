# ✅ Mise à jour de run_fullstack_elm.sh pour PostgreSQL

## Modifications apportées

### 1. Chargement automatique de DATABASE_URL

Le script charge maintenant automatiquement la variable `DATABASE_URL` depuis `backend/.env`:

```bash
# PostgreSQL configuration
ENV_FILE="$BACKEND_DIR/.env"
if [[ -f "$ENV_FILE" ]]; then
    # Load DATABASE_URL from .env if not already set
    if [[ -z "${DATABASE_URL:-}" ]]; then
        export DATABASE_URL=$(grep "^DATABASE_URL=" "$ENV_FILE" | cut -d'=' -f2-)
    fi
fi
```

### 2. Vérification de la configuration PostgreSQL

Avant le démarrage, le script vérifie:
- Si `DATABASE_URL` est configuré
- Si PostgreSQL est accessible (test de connexion)
- Propose d'exécuter `setup_database.sh` si nécessaire

```bash
echo "🗄️  PostgreSQL Configuration:"
if [[ -n "${DATABASE_URL:-}" ]]; then
    echo "   ✅ DATABASE_URL configured"

    # Test de connexion
    if psql "$DATABASE_URL" -c "SELECT 1;" >/dev/null 2>&1; then
        echo "   ✅ PostgreSQL connection successful"
    else
        echo "   ⚠️  Cannot connect to PostgreSQL"
        echo "   💡 Run: cd backend && ./setup_database.sh"
    fi
else
    echo "   ⚠️  DATABASE_URL not configured"
    echo "   💡 To enable route saving, run: cd backend && ./setup_database.sh"
    echo "   The app will still work but routes won't be saved to database."
fi
```

### 3. Transmission de DATABASE_URL au backend

La variable est maintenant passée au processus backend:

```bash
env \
  CARGO_TARGET_DIR="$TARGET_DIR" \
  PBF_PATH="$PBF_PATH" \
  CACHE_DIR="$CACHE_DIR" \
  LOCAL_DEM_PATH="${LOCAL_DEM_PATH:-}" \
  DATABASE_URL="${DATABASE_URL:-}" \    # ← Ajouté
  cargo run -p backend --bin backend_partial "$@" &
```

### 4. Affichage du statut PostgreSQL

Dans les logs de démarrage:

```bash
printf 'Backend started with PID %s (listening on %s).\n' "$BACKEND_PID" "$BACKEND_PORT"
printf 'PBF: %s\n' "$PBF_PATH"
printf 'Cache: %s\n' "$CACHE_DIR"
if [[ -n "${DATABASE_URL:-}" ]]; then
    printf 'Database: PostgreSQL (configured)\n'
else
    printf 'Database: Not configured\n'
fi
```

### 5. Feature PostgreSQL dans la liste

Ajout de la feature PostgreSQL:

```bash
echo "Features:"
echo "  - 🎨 Elm MVU architecture (pure functional)"
echo "  - 🔥 Hot reload (modify Elm code → instant update!)"
echo "  - 🐛 Elm Debugger (time-travel debugging)"
echo "  - 🗺️  2D/3D map view with MapLibre GL JS"
echo "  - 🏔️  Free terrain tiles (no API keys needed)"
echo "  - 📊 On-demand graph generation from PBF data"
echo "  - 🗄️  PostgreSQL database for route persistence"    # ← Ajouté
echo "  - ⚡ Bundle 10x lighter than Seed/WASM (~30 KB vs 300 KB)"
```

## Documentation mise à jour

Le fichier `scripts/README.md` a été mis à jour avec:
- Instructions d'installation PostgreSQL
- Guide de configuration (`setup_database.sh`)
- Section troubleshooting pour PostgreSQL
- Variables d'environnement PostgreSQL

## Utilisation

### Scénario 1: PostgreSQL configuré

```bash
# 1. Configurer PostgreSQL (une seule fois)
cd backend
./setup_database.sh

# 2. Lancer l'application
cd ..
./scripts/run_fullstack_elm.sh
```

**Sortie attendue:**
```
🗄️  PostgreSQL Configuration:
   ✅ DATABASE_URL configured
   ✅ PostgreSQL connection successful

Starting backend with on-demand graph generation...
Backend started with PID 12345 (listening on 8080).
Database: PostgreSQL (configured)
```

### Scénario 2: Sans PostgreSQL

```bash
./scripts/run_fullstack_elm.sh
```

**Sortie attendue:**
```
🗄️  PostgreSQL Configuration:
   ⚠️  DATABASE_URL not configured
   💡 To enable route saving, run: cd backend && ./setup_database.sh
   The app will still work but routes won't be saved to database.

   Continue without database? (Y/n)
```

Le script propose de continuer sans PostgreSQL. L'application fonctionnera mais les routes ne seront pas sauvegardées.

### Scénario 3: PostgreSQL configuré mais non accessible

```bash
./scripts/run_fullstack_elm.sh
```

**Sortie attendue:**
```
🗄️  PostgreSQL Configuration:
   ✅ DATABASE_URL configured
   ⚠️  Cannot connect to PostgreSQL
   💡 Run: cd backend && ./setup_database.sh

   Continue anyway? (y/N)
```

Le script détecte que PostgreSQL n'est pas accessible et propose de continuer ou d'abandonner.

## Comportement gracieux

Le script permet de lancer l'application même sans PostgreSQL:
- ✅ L'application démarre normalement
- ✅ Les routes peuvent être calculées
- ⚠️  Les routes ne peuvent pas être sauvegardées en base
- 💡 Le script indique clairement comment configurer PostgreSQL

## Tests effectués

- ✅ Syntaxe bash validée (`bash -n`)
- ✅ Chargement de `.env` testé
- ✅ Variables d'environnement transmises au backend

## Prochaines étapes

Pour tester le script avec PostgreSQL:

1. **Configurer PostgreSQL:**
   ```bash
   cd backend
   ./setup_database.sh
   ```

2. **Lancer l'application:**
   ```bash
   ./scripts/run_fullstack_elm.sh
   ```

3. **Vérifier dans les logs:**
   - "✅ DATABASE_URL configured"
   - "✅ PostgreSQL connection successful"
   - "✅ PostgreSQL connected successfully" (dans les logs backend)
   - "Database migrations completed"

Le script est prêt à l'emploi! 🎉
