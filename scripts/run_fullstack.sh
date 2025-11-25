#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/target"
FRONTEND_DIR="$ROOT_DIR/frontend"
FRONTEND_PORT="8081"
BACKEND_PORT="8080"

# PBF data for routing graph
DEFAULT_PBF="$ROOT_DIR/backend/data/rhone-alpes-251111.osm.pbf"
export PBF_PATH="${GRAPH_PBF:-$DEFAULT_PBF}"
export CACHE_DIR="${CACHE_DIR:-data/cache}"

mkdir -p "$TARGET_DIR"
cd "$ROOT_DIR"

# Cleanup handler
kill_child_processes() {
    if [[ -n "${BACKEND_PID:-}" ]]; then
        kill "$BACKEND_PID" 2>/dev/null || true
    fi
    if [[ -n "${FRONTEND_PID:-}" ]]; then
        kill "$FRONTEND_PID" 2>/dev/null || true
    fi
}

trap kill_child_processes EXIT

# Free port if occupied
free_port() {
    local port="$1"
    local pids
    pids=$(lsof -ti tcp:"$port" || true)
    if [[ -n "$pids" ]]; then
        echo "Port $port busy (PIDs: $pids). Terminating..."
        kill "$pids" 2>/dev/null || true
        sleep 1
    fi
}

# Ensure ports are available
free_port "$BACKEND_PORT"
free_port "$FRONTEND_PORT"

# Start backend with on-demand graph generation
echo "Starting backend with on-demand graph generation..."
env \
  CARGO_TARGET_DIR="$TARGET_DIR" \
  PBF_PATH="$PBF_PATH" \
  CACHE_DIR="$CACHE_DIR" \
  cargo run -p backend --bin backend_partial "$@" &
BACKEND_PID=$!

printf 'Backend started with PID %s (listening on %s).\n' "$BACKEND_PID" "$BACKEND_PORT"
printf 'PBF: %s\n' "$PBF_PATH"
printf 'Cache: %s\n' "$CACHE_DIR"

# Build frontend with Maplibre GL JS
echo "Building frontend with Maplibre GL JS and wasm-pack..."
(cd "$FRONTEND_DIR" && ./build.sh)

# Start frontend dev server
echo "Starting frontend dev server on http://localhost:$FRONTEND_PORT ..."
(cd "$FRONTEND_DIR/dist" && python3 -m http.server "$FRONTEND_PORT") &
FRONTEND_PID=$!

printf 'Frontend server started with PID %s on http://localhost:%s\n' "$FRONTEND_PID" "$FRONTEND_PORT"
echo ""
echo "✅ Application ready!"
echo "   Frontend: http://localhost:$FRONTEND_PORT"
echo "   Backend:  http://localhost:$BACKEND_PORT"
echo ""
echo "Features:"
echo "  - 2D/3D map view with Maplibre GL JS"
echo "  - Free terrain tiles (no API keys needed)"
echo "  - On-demand graph generation from PBF data"
echo ""
echo "Press Ctrl+C to stop all services."

wait "$FRONTEND_PID"
