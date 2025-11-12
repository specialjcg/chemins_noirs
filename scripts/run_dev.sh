#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/target"

mkdir -p "$TARGET_DIR"
cd "$ROOT_DIR"
exec env CARGO_TARGET_DIR="$TARGET_DIR" cargo run -p backend --bin backend "$@"
