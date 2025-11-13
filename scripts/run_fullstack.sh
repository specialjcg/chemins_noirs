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

generate_graph() {
    if [[ -f "$GRAPH_JSON" ]]; then
        echo "Using existing graph at $GRAPH_JSON"
        return
    fi
    if [[ -z "$GRAPH_PBF" || ! -f "$GRAPH_PBF" ]]; then
        echo "PBF file '$GRAPH_PBF' not found. Falling back to sample graph."
        GRAPH_JSON="$ROOT_DIR/backend/data/sample_graph.json"
        return
    fi
    echo "Generating graph JSON at $GRAPH_JSON from $GRAPH_PBF ..."
    env CARGO_TARGET_DIR="$TARGET_DIR" cargo run -p backend --bin build_graph -- \
        --pbf "$GRAPH_PBF" \
        --output "$GRAPH_JSON" \
        --min-lat "$GRAPH_MIN_LAT" \
        --max-lat "$GRAPH_MAX_LAT" \
        --min-lon "$GRAPH_MIN_LON" \
        --max-lon "$GRAPH_MAX_LON"
}

generate_graph
export GRAPH_JSON

if lsof -iTCP:"$BACKEND_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
    printf 'Port %s already in use; not starting backend.\n' "$BACKEND_PORT"
else
    env CARGO_TARGET_DIR="$TARGET_DIR" cargo run -p backend --bin backend "$@" &
    BACKEND_PID=$!
    printf 'Backend started with PID %s (listening on %s).\n' "$BACKEND_PID" "$BACKEND_PORT"
fi

if lsof -iTCP:"$FRONTEND_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
    echo "Port $FRONTEND_PORT already in use; aborting."
    exit 1
fi

echo "Starting frontend dev server on http://localhost:$FRONTEND_PORT ..."
(cd "$FRONTEND_DIR" && trunk serve --port "$FRONTEND_PORT" --open) &
FRONTEND_PID=$!

wait "$FRONTEND_PID"
