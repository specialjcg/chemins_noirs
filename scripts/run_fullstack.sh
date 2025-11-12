#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/target"
FRONTEND_DIR="$ROOT_DIR/frontend"
FRONTEND_PORT="8081"
BACKEND_PORT="8080"

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

env CARGO_TARGET_DIR="$TARGET_DIR" cargo run -p backend --bin backend "$@" &
BACKEND_PID=$!

echo "Backend started with PID $BACKEND_PID (listening on $BACKEND_PORT)"

echo "Starting frontend dev server on http://localhost:$FRONTEND_PORT ..."
(cd "$FRONTEND_DIR" && trunk serve --port "$FRONTEND_PORT" --open) &
FRONTEND_PID=$!

wait "$BACKEND_PID"
