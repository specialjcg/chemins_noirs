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
