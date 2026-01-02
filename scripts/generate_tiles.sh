#!/bin/bash

# Tile Generation Script for Chemins Noirs
# Pre-generates 20km×20km tiles from PBF for ultra-fast graph loading (<10s)

set -e

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BACKEND_DIR="$ROOT_DIR/backend"

cd "$BACKEND_DIR"

# Default values
PBF_PATH="${PBF_PATH:-data/rhone-alpes-251111.osm.pbf}"
TILES_DIR="${TILES_DIR:-data/tiles}"
TILE_SIZE="${TILE_SIZE:-20}"

echo "🔧 Tile Generation Configuration:"
echo "  PBF file: $PBF_PATH"
echo "  Output directory: $TILES_DIR"
echo "  Tile size: ${TILE_SIZE}km × ${TILE_SIZE}km"
echo ""

# Check if PBF file exists
if [ ! -f "$PBF_PATH" ]; then
    echo "❌ Error: PBF file not found: $PBF_PATH"
    echo "   Please set PBF_PATH environment variable or place file at default location"
    exit 1
fi

# Create tiles directory
mkdir -p "$TILES_DIR"

echo "📊 Starting tile generation..."
echo "⏱️  This will take approximately 30-60 minutes for Rhône-Alpes"
echo "💡 Tip: The process can be interrupted and resumed (existing tiles are skipped)"
echo ""

# Run tile generation
cargo run --release --bin generate_tiles -- \
    --pbf "$PBF_PATH" \
    --output "$TILES_DIR" \
    --tile-size "$TILE_SIZE"

echo ""
echo "🎉 Tile generation complete!"
echo ""
echo "📝 Next steps:"
echo "  1. Set the tiles directory: export TILES_DIR=$TILES_DIR"
echo "  2. Start the backend: ./scripts/run_fullstack_elm.sh"
echo "  3. Enjoy <10s route generation! 🚀"
