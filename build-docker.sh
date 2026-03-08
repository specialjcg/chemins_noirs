#!/bin/bash
#
# Build Chemins Noirs dans un container Docker Ubuntu 22.04
# Compatible avec les VPS Ubuntu 22.04 (GLIBC 2.35)
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
cd "$SCRIPT_DIR"

IMAGE_NAME="chemins-noirs-builder"
CONTAINER_NAME="chemins-noirs-build"

echo "=== Build Chemins Noirs (Docker Ubuntu 22.04) ==="

# Étape 1: Construire l'image Docker (si nécessaire)
if ! docker images | grep -q "$IMAGE_NAME"; then
    echo "[1/3] Construction de l'image Docker (première fois, ~5 min)..."
    docker build --network host -f Dockerfile.build -t "$IMAGE_NAME" .
else
    echo "[1/3] Image Docker existante, skip..."
fi

# Étape 2: Compiler le backend Rust dans le container
echo "[2/3] Compilation du backend Rust (release, ~5-10 min)..."

mkdir -p target-docker

docker run --rm \
    --network host \
    -e CARGO_TARGET_DIR=/app/target-docker \
    -e CARGO_HOME=/root/.cargo \
    -v "$SCRIPT_DIR:/app" \
    -v cargo-cache-chemins:/root/.cargo/registry \
    -w /app \
    "$IMAGE_NAME" \
    bash -c "cargo build --release -p backend --bin backend_partial 2>&1 | tail -50"

# Vérifier que le binaire existe
if [ ! -f "target-docker/release/backend_partial" ]; then
    echo "ERREUR: Le binaire n'a pas été créé"
    exit 1
fi

# Copier vers target/release pour compatibilité avec deploy.sh
mkdir -p target/release
cp target-docker/release/backend_partial target/release/

echo "[OK] Backend compilé: $(du -h target/release/backend_partial | cut -f1)"

# Étape 3: Compiler le frontend Elm (local, pas besoin de Docker)
echo "[3/3] Compilation du frontend Elm..."
cd frontend-elm
if [ ! -d "node_modules" ]; then
    npm install
fi
npm run build
cd ..
echo "[OK] Frontend compilé dans frontend-elm/dist/"

echo ""
echo "=== Build terminé ==="
echo "Binaire:  target/release/backend_partial"
echo "Frontend: frontend-elm/dist/"
echo ""
echo "Pour déployer: ./deploy.sh deploy"
