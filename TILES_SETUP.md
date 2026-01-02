# Tile-Based Graph Loading - Setup Guide

## Overview

The tile-based system dramatically improves graph generation performance:

- **Without tiles**: ~2 minutes for first route (reads 483MB PBF file)
- **With tiles**: <10 seconds for first route (loads only relevant 20km×20km tiles)

## How It Works

1. **Pre-generation**: The Rhône-Alpes region is divided into 20km×20km tiles
2. **On-demand loading**: Only tiles overlapping the route bbox are loaded
3. **Caching**: Merged graphs are still cached for instant subsequent requests

## Setup Instructions

### Step 1: Generate Tiles (One-time setup, ~30-60 minutes)

Run the tile generation script:

```bash
./scripts/generate_tiles.sh
```

This will:
- Read the PBF file: `backend/data/rhone-alpes-251111.osm.pbf`
- Generate tiles in: `backend/data/tiles/`
- Create ~100-200 tile files (compressed .zst format)
- Skip empty tiles (areas with no roads)

**Progress tracking:**
- The tool shows progress every 10 tiles
- If interrupted (Ctrl+C), you can resume - existing tiles are skipped
- Estimated time: 1-2 minutes per tile × number of non-empty tiles

### Step 2: Configure Backend

The fullstack script automatically detects tiles if they exist in `backend/data/tiles/`.

Alternatively, set the environment variable:

```bash
export TILES_DIR=/path/to/tiles
```

### Step 3: Run Backend

```bash
./scripts/run_fullstack_elm.sh
```

You should see in the logs:
```
🚀 Tiles directory found: backend/data/tiles (FAST MODE enabled - <10s per route)
```

## Performance Comparison

### Without Tiles (PBF mode)
```
21:22:15 - Request received
21:24:11 - Route generated (1min 56s)
```

### With Tiles (Fast mode)
```
21:22:15 - Request received
21:22:23 - Route generated (8 seconds) ✨
```

## Tile Generation Options

### Custom Configuration

```bash
# Use different PBF file
PBF_PATH=data/custom-region.osm.pbf ./scripts/generate_tiles.sh

# Use different output directory
TILES_DIR=data/custom-tiles ./scripts/generate_tiles.sh

# Use different tile size (default: 20km)
TILE_SIZE=30 ./scripts/generate_tiles.sh
```

### Manual Generation

```bash
cd backend
cargo run --release --bin generate_tiles -- \
    --pbf data/rhone-alpes-251111.osm.pbf \
    --output data/tiles \
    --tile-size 20
```

## Tile Storage

Each tile is stored as a compressed JSON file:

- **Format**: `tile_X_Y.json.zst` (Zstandard compression)
- **Size**: 50KB - 5MB per tile (depending on road density)
- **Total size**: ~500MB - 2GB for Rhône-Alpes region

## Fallback Behavior

The system automatically falls back to PBF mode if:
1. `TILES_DIR` is not set
2. Tiles directory doesn't exist
3. Required tiles are missing (with warning logged)

## Troubleshooting

### "Tile not found" warnings

If you see warnings like:
```
WARN Tile not found: data/tiles/tile_42_408.json.zst, skipping
```

This means:
- The route bbox extends into an area without tiles
- The system falls back to available tiles or PBF mode
- To fix: Re-run tile generation to cover the full region

### Incomplete tile generation

If tile generation was interrupted:
- Simply re-run `./scripts/generate_tiles.sh`
- Existing tiles are automatically skipped
- Only missing tiles will be generated

### Memory usage

Tile generation is memory-intensive (loads entire PBF multiple times):
- Recommended: 8GB+ RAM
- If out of memory: Reduce `TILE_SIZE` to 10km

## Advanced: Tile Grid Details

The tile system uses a grid coordinate system:

- **Tile ID**: `(x, y)` integers
- **Origin**: Based on lat/lon (0, 0)
- **Size**: 20km × 20km (default)
- **Overlap**: Tiles may overlap at edges (to ensure graph connectivity)

Example for Lyon area (45.76°N, 4.84°E):
```
Tile (-1, 412): 45.7° - 45.88°N, 4.72° - 4.91°E
```

## Benefits

✅ **Fast first request**: <10s instead of ~2min
✅ **Scalable**: Load only what's needed
✅ **Resumable**: Interrupted generation can resume
✅ **Compressed**: 60-70% space savings with Zstandard
✅ **Automatic fallback**: Works with or without tiles

## Next Steps

After setup, your routes will load in <10 seconds! 🚀

For further optimizations, see [PERFORMANCE_OPTIMIZATIONS.md](PERFORMANCE_OPTIMIZATIONS.md).
