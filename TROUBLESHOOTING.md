# 🔧 Troubleshooting - Chemins Noirs

## Erreur : `proj-sys` build failure

### Symptômes

```
error: failed to run custom build command for `proj-sys v0.23.2`
pkg-config unable to find existing libproj installation
The pkg-config command could not be found.
is `cmake` not installed?
```

### Cause

Le backend Rust utilise la bibliothèque `proj-sys` pour les calculs de projections géographiques. Cette bibliothèque nécessite :

1. **pkg-config** - Outil pour configurer les bibliothèques système
2. **cmake** - Outil de build pour compiler PROJ depuis les sources
3. **libsqlite3-dev** - Dépendance de PROJ

### Solution

#### Ubuntu/Debian

```bash
sudo apt update
sudo apt install -y pkg-config cmake libsqlite3-dev build-essential
```

#### Fedora/RHEL/CentOS

```bash
sudo yum install -y pkg-config cmake sqlite-devel gcc gcc-c++
```

#### Arch Linux

```bash
sudo pacman -S pkg-config cmake sqlite
```

#### macOS

```bash
brew install pkg-config cmake sqlite3
```

### Vérification

Après installation, vérifier que les outils sont disponibles :

```bash
pkg-config --version
cmake --version
```

### Rebuild

Une fois les dépendances installées :

```bash
cd backend
cargo clean
cargo build
```

## Autres erreurs potentielles

### Erreur : `libsqlite3` not found

**Solution** :
```bash
sudo apt install libsqlite3-dev  # Ubuntu/Debian
sudo yum install sqlite-devel    # Fedora/RHEL
```

### Erreur : Frontend Elm - `elm` command not found

**Solution** :
```bash
npm install -g elm elm-format elm-test
```

### Erreur : Frontend - MapLibre not loading

**Vérifications** :
1. `public/maplibre_map.js` existe
2. Console navigateur (F12) pour erreurs JS
3. Backend tourne sur port 8080

### Erreur : CORS when calling API

**Solution** :

Vérifier que Vite proxy est configuré dans `vite.config.js` :

```javascript
export default defineConfig({
  server: {
    proxy: {
      '/api': {
        target: 'http://localhost:8080',
        changeOrigin: true
      }
    }
  }
});
```

### Erreur : Port 3000 or 8080 already in use

**Solution** :

```bash
# Trouver le processus
lsof -i :3000
lsof -i :8080

# Tuer le processus
kill -9 <PID>

# Ou utiliser un autre port
PORT=3001 npm run dev  # Frontend
BACKEND_PORT=8081 cargo run  # Backend
```

## Support

Si les problèmes persistent :

1. **Vérifier les logs complets** : `RUST_BACKTRACE=full cargo build`
2. **Nettoyer le cache** : `cargo clean`
3. **Vérifier la version de Rust** : `rustc --version` (min 1.70+)
4. **Vérifier la version de Node** : `node --version` (min 18+)
