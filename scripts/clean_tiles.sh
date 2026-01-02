#!/bin/bash

# Clean invalid tiles

set -e

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TILES_DIR="${TILES_DIR:-$ROOT_DIR/backend/data/tiles}"

echo "🧹 Cleaning invalid tiles from: $TILES_DIR"

if [ ! -d "$TILES_DIR" ]; then
    echo "ℹ️  No tiles directory found - nothing to clean"
    exit 0
fi

# Count tiles
TILE_COUNT=$(find "$TILES_DIR" -name "*.zst" 2>/dev/null | wc -l)

if [ "$TILE_COUNT" -eq 0 ]; then
    echo "ℹ️  No tiles found - nothing to clean"
    exit 0
fi

echo "Found $TILE_COUNT tiles to delete"
echo ""
read -p "⚠️  Delete all tiles? (y/N) " -n 1 -r
echo

if [[ $REPLY =~ ^[Yy]$ ]]; then
    rm -rf "$TILES_DIR"/*.zst
    echo "✅ Deleted all tiles"
    echo ""
    echo "Next steps:"
    echo "  1. Rebuild: cd backend && cargo build --release --bin generate_tiles"
    echo "  2. Regenerate: ./scripts/generate_tiles.sh"
else
    echo "❌ Cancelled"
fi
