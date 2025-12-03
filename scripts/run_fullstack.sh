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
DEFAULT_DEM_TIF="$ROOT_DIR/backend/data/dem/region.tif"
DEFAULT_DEM_ASC="$ROOT_DIR/backend/data/dem/region.asc"

if [[ -z "${LOCAL_DEM_PATH:-}" ]]; then
    if [[ -f "$DEFAULT_DEM_ASC" ]]; then
        LOCAL_DEM_PATH="$DEFAULT_DEM_ASC"
    elif [[ -f "$DEFAULT_DEM_TIF" ]]; then
        LOCAL_DEM_PATH="$DEFAULT_DEM_TIF"
    fi
fi

if [[ -n "${LOCAL_DEM_PATH:-}" ]]; then
    if [[ "${LOCAL_DEM_PATH##*.}" != "asc" ]]; then
        if command -v gdal_translate >/dev/null 2>&1; then
            dem_source="$LOCAL_DEM_PATH"
            dem_ascii="${dem_source%.*}.asc"
            if [[ ! -f "$dem_ascii" || "$dem_ascii" -ot "$dem_source" ]]; then
                echo "Converting $dem_source to Arc/Info ASCII Grid ($dem_ascii)..."
                gdal_translate -of AAIGrid "$dem_source" "$dem_ascii" >/dev/null
            fi
            LOCAL_DEM_PATH="$dem_ascii"
        else
            echo "gdal_translate not found; cannot convert $LOCAL_DEM_PATH to ASCII grid. Falling back to Open-Meteo."
            unset LOCAL_DEM_PATH
        fi
    fi
fi

if [[ -n "${LOCAL_DEM_PATH:-}" ]]; then
    echo "Using local DEM grid at $LOCAL_DEM_PATH"
    export LOCAL_DEM_PATH
else
    echo "No usable local DEM grid detected; elevation will use Open-Meteo."
fi

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
  LOCAL_DEM_PATH="${LOCAL_DEM_PATH:-}" \
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
