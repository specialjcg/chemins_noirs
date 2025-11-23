#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/target"
FRONTEND_DIR="$ROOT_DIR/frontend"
FRONTEND_PORT="8081"
BACKEND_PORT="8080"

DEFAULT_PBF="$ROOT_DIR/backend/data/rhone-alpes-251111.osm.pbf"
GRAPH_JSON="${GRAPH_JSON:-$ROOT_DIR/backend/data/generated_graph.json}"
GRAPH_PBF="${GRAPH_PBF:-$DEFAULT_PBF}"
GRAPH_MIN_LAT="${GRAPH_MIN_LAT:-44.5}"
GRAPH_MAX_LAT="${GRAPH_MAX_LAT:-46.6}"
GRAPH_MIN_LON="${GRAPH_MIN_LON:-4.0}"
GRAPH_MAX_LON="${GRAPH_MAX_LON:-6.5}"

mkdir -p "$TARGET_DIR"
cd "$ROOT_DIR"

kill_child_processes() {
    if [[ -n "${BACKEND_PID:-}" ]]; then
        kill "$BACKEND_PID" 2>/dev/null || true
    fi
    if [[ -n "${FRONTEND_PID:-}" ]]; then
        kill "$FRONTEND_PID" 2>/dev/null || true
    fi
}

trap kill_child_processes EXIT

free_port() {
    local port="$1"
    local pids
    pids=$(lsof -ti tcp:"$port" || true)
    if [[ -n "$pids" ]]; then
        echo "Port $port busy (PIDs: $pids). Terminating..."
        kill $pids 2>/dev/null || true
        sleep 1
    fi
}

graph_has_nodes() {
    # Pure shell + jq solution (faster than Python)
    if ! command -v jq &>/dev/null; then
        # Fallback: check if file is non-empty and contains "nodes"
        [[ -s "$1" ]] && grep -q '"nodes"' "$1"
        return $?
    fi
    # Use jq to check if nodes array exists and has elements
    local node_count
    node_count=$(jq -e '.nodes | length' "$1" 2>/dev/null) || return 1
    [[ "$node_count" -gt 0 ]]
}

generate_graph() {
    if [[ -f "$GRAPH_JSON" ]]; then
        if graph_has_nodes "$GRAPH_JSON"; then
            echo "Using existing graph at $GRAPH_JSON"
            return
        else
            echo "Existing graph at $GRAPH_JSON is empty; regenerating..."
            rm -f "$GRAPH_JSON"
        fi
    fi
    if [[ -z "$GRAPH_PBF" || ! -f "$GRAPH_PBF" ]]; then
        echo "PBF file '$GRAPH_PBF' not found. Falling back to sample graph."
        GRAPH_JSON="$ROOT_DIR/backend/data/sample_graph.json"
        return
    fi
    echo "Generating graph JSON at $GRAPH_JSON from $GRAPH_PBF ..."
    env CARGO_TARGET_DIR="$TARGET_DIR" cargo run --release -p backend --bin build_graph -- \
        --pbf "$GRAPH_PBF" \
        --output "$GRAPH_JSON" \
        --min-lat "$GRAPH_MIN_LAT" \
        --max-lat "$GRAPH_MAX_LAT" \
        --min-lon "$GRAPH_MIN_LON" \
        --max-lon "$GRAPH_MAX_LON"
}

# Using backend_partial with on-demand graph generation - no pre-generation needed!
export PBF_PATH="${GRAPH_PBF:-data/rhone-alpes-251111.osm.pbf}"
export CACHE_DIR="${CACHE_DIR:-data/cache}"

free_port "$BACKEND_PORT"
free_port "$FRONTEND_PORT"

echo "Starting backend_partial with on-demand graph generation..."
env CARGO_TARGET_DIR="$TARGET_DIR" PBF_PATH="$PBF_PATH" CACHE_DIR="$CACHE_DIR" cargo run -p backend --bin backend_partial "$@" &
BACKEND_PID=$!
printf 'Backend_partial started with PID %s (listening on %s).\n' "$BACKEND_PID" "$BACKEND_PORT"
printf 'PBF: %s\n' "$PBF_PATH"
printf 'Cache: %s\n' "$CACHE_DIR"

echo "Building frontend with wasm-pack..."
(cd "$FRONTEND_DIR" && ./build.sh)

echo "Starting frontend dev server on http://localhost:$FRONTEND_PORT ..."
(cd "$FRONTEND_DIR/dist" && python3 -m http.server "$FRONTEND_PORT") &
FRONTEND_PID=$!
printf 'Frontend server started with PID %s on http://localhost:%s\n' "$FRONTEND_PID" "$FRONTEND_PORT"

wait "$FRONTEND_PID"
