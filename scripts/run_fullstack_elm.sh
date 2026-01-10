#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/target"
FRONTEND_DIR="$ROOT_DIR/frontend-elm"
BACKEND_DIR="$ROOT_DIR/backend"
FRONTEND_PORT="3000"  # Vite dev server port
BACKEND_PORT="8080"

# PBF data for routing graph
DEFAULT_PBF="$ROOT_DIR/backend/data/rhone-alpes-251111.osm.pbf"
export PBF_PATH="${GRAPH_PBF:-$DEFAULT_PBF}"
export CACHE_DIR="${CACHE_DIR:-data/cache}"

# Tiles directory for fast graph loading (<10s instead of ~2min)
DEFAULT_TILES="$ROOT_DIR/backend/data/tiles"
if [[ -d "$DEFAULT_TILES" ]] || [[ -n "${TILES_DIR:-}" ]]; then
    export TILES_DIR="${TILES_DIR:-$DEFAULT_TILES}"
fi
DEFAULT_DEM_TIF="$ROOT_DIR/backend/data/dem/region.tif"
DEFAULT_DEM_ASC="$ROOT_DIR/backend/data/dem/region.asc"

# PostgreSQL configuration
ENV_FILE="$BACKEND_DIR/.env"
if [[ -f "$ENV_FILE" ]]; then
    # Load DATABASE_URL from .env if not already set
    if [[ -z "${DATABASE_URL:-}" ]]; then
        export DATABASE_URL=$(grep "^DATABASE_URL=" "$ENV_FILE" | cut -d'=' -f2-)
    fi
fi

# DEM setup (same as original script)
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
        echo "Stopping backend (PID $BACKEND_PID)..."
        kill "$BACKEND_PID" 2>/dev/null || true
    fi
    if [[ -n "${FRONTEND_PID:-}" ]]; then
        echo "Stopping frontend (PID $FRONTEND_PID)..."
        kill "$FRONTEND_PID" 2>/dev/null || true
    fi
}

trap kill_child_processes EXIT

# Free port if occupied
free_port() {
    local port="$1"
    local pids
    pids=$(lsof -ti tcp:"$port" 2>/dev/null || true)
    if [[ -n "$pids" ]]; then
        echo "Port $port busy (PIDs: $pids). Terminating..."
        kill $pids 2>/dev/null || true
        sleep 1
    fi
}

# Ensure ports are available
free_port "$BACKEND_PORT"
free_port "$FRONTEND_PORT"

# Check PostgreSQL configuration
echo ""
echo "🗄️  PostgreSQL Configuration:"
if [[ -n "${DATABASE_URL:-}" ]]; then
    echo "   ✅ DATABASE_URL configured"

    # Test PostgreSQL connection
    if command -v psql >/dev/null 2>&1; then
        # Extract connection details from DATABASE_URL
        # Format: postgresql://user:password@host/database
        if echo "$DATABASE_URL" | grep -q "postgresql://"; then
            db_check=$(echo "$DATABASE_URL" | sed 's|postgresql://||' | sed 's|:.*@| -h |' | sed 's|/| -d |' | awk '{print $1}')
            if psql "$DATABASE_URL" -c "SELECT 1;" >/dev/null 2>&1; then
                echo "   ✅ PostgreSQL connection successful"
            else
                echo "   ⚠️  Cannot connect to PostgreSQL"
                echo "   💡 Run: cd backend && ./setup_database.sh"
                read -p "   Continue anyway? (y/N) " -n 1 -r
                echo
                if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                    exit 1
                fi
            fi
        fi
    else
        echo "   ⚠️  psql not found, skipping connection test"
    fi
else
    echo "   ⚠️  DATABASE_URL not configured"
    echo "   💡 To enable route saving, run: cd backend && ./setup_database.sh"
    echo "   The app will still work but routes won't be saved to database."
    echo ""
    read -p "   Continue without database? (Y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Nn]$ ]]; then
        exit 1
    fi
fi
echo ""

# Check if Elm frontend dependencies are installed
if [[ ! -d "$FRONTEND_DIR/node_modules" ]]; then
    echo "Installing Elm frontend dependencies..."
    (cd "$FRONTEND_DIR" && npm install)
fi

# Check if Elm is installed
if ! command -v elm &> /dev/null; then
    echo "⚠️  Warning: 'elm' command not found. Installing globally..."
    npm install -g elm elm-format elm-test
fi

# Start backend with on-demand graph generation
echo "Starting backend with on-demand graph generation..."
env \
  CARGO_TARGET_DIR="$TARGET_DIR" \
  PBF_PATH="$PBF_PATH" \
  CACHE_DIR="$CACHE_DIR" \
  LOCAL_DEM_PATH="${LOCAL_DEM_PATH:-}" \
  DATABASE_URL="${DATABASE_URL:-}" \
  cargo run -p backend --bin backend_partial "$@" &
BACKEND_PID=$!

printf 'Backend started with PID %s (listening on %s).\n' "$BACKEND_PID" "$BACKEND_PORT"
printf 'PBF: %s\n' "$PBF_PATH"
printf 'Cache: %s\n' "$CACHE_DIR"
if [[ -n "${DATABASE_URL:-}" ]]; then
    printf 'Database: PostgreSQL (configured)\n'
else
    printf 'Database: Not configured\n'
fi

# Wait a bit for backend to start
sleep 2

# Start Elm frontend in development mode (with proxy)
echo "Starting Elm frontend development server on http://localhost:$FRONTEND_PORT ..."
(cd "$FRONTEND_DIR" && npm run dev) &
FRONTEND_PID=$!

printf 'Elm frontend dev server started with PID %s on http://localhost:%s\n' "$FRONTEND_PID" "$FRONTEND_PORT"
echo ""
echo "✅ Application ready!"
echo "   Frontend (Elm): http://localhost:$FRONTEND_PORT"
echo "   Backend (Rust): http://localhost:$BACKEND_PORT"
echo ""
echo "Features:"
echo "  - 🎨 Elm MVU architecture (pure functional)"
echo "  - 🔧 Development mode (hot reload, Elm debugger)"
echo "  - 🗺️  2D/3D map view with MapLibre GL JS"
echo "  - 🏔️  Free terrain tiles (no API keys needed)"
echo "  - 📊 On-demand graph generation from PBF data"
echo "  - 🗄️  PostgreSQL database for route persistence"
echo "  - 🔄 API proxy configured (port 3000 → 8080)"
echo "  - ⚡ Fast routing with 1km margin optimization"
echo ""
echo "📖 Documentation:"
echo "   - Quick Start:     $ROOT_DIR/QUICK_START.md"
echo "   - Frontend README: $FRONTEND_DIR/README.md"
echo "   - Testing Guide:   $ROOT_DIR/backend/TESTING.md"
echo "   - Improvements:    $ROOT_DIR/IMPROVEMENTS_SUMMARY.md"
echo ""
echo "Press Ctrl+C to stop all services."

wait "$FRONTEND_PID"
