#!/usr/bin/env bash
#
# Run backend with on-demand partial graph generation
# This avoids loading the massive 4.2GB graph into memory
#
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# Configuration
PBF_PATH="${PBF_PATH:-data/rhone-alpes-251111.osm.pbf}"
CACHE_DIR="${CACHE_DIR:-data/cache}"
BACKEND_PORT="${BACKEND_PORT:-8080}"

echo "🚀 Starting backend with partial graph generation"
echo "📍 PBF file: $PBF_PATH"
echo "💾 Cache directory: $CACHE_DIR"
echo "🌐 Port: $BACKEND_PORT"

# Create cache directory
mkdir -p "$CACHE_DIR"

# Kill any existing backend on the port
if lsof -ti tcp:$BACKEND_PORT >/dev/null 2>&1; then
    echo "⚠️  Port $BACKEND_PORT is busy, killing existing process..."
    kill $(lsof -ti tcp:$BACKEND_PORT) 2>/dev/null || true
    sleep 1
fi

# Export configuration
export PBF_PATH
export CACHE_DIR

# Run the partial backend (no pre-loading!)
echo "✅ Starting backend (no graph pre-loading, generates on-demand)"
cargo run --bin backend_partial
