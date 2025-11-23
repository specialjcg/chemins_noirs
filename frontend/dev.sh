#!/bin/bash
# Development server with auto-reload (replaces trunk serve)

set -e

# Build once
echo "🔨 Initial build..."
./build.sh

# Start Python HTTP server in background
echo "🌐 Starting dev server on http://localhost:8081"
cd dist && python3 -m http.server 8081 &
SERVER_PID=$!
cd ..

# Cleanup function
cleanup() {
    echo "🛑 Stopping dev server..."
    kill $SERVER_PID 2>/dev/null || true
    exit 0
}

trap cleanup INT TERM

echo "✅ Dev server running at http://localhost:8081"
echo "📝 Watching for changes... (Ctrl+C to stop)"
echo ""

# Watch for file changes and rebuild
while true; do
    # Watch Rust files and rebuild on change
    inotifywait -q -r -e modify,create,delete src/*.rs lib.rs Cargo.toml 2>/dev/null && {
        echo "🔄 Changes detected, rebuilding..."
        ./build.sh
        echo "✅ Rebuild complete!"
    }

    # Also watch JS files
    inotifywait -q -e modify three3d_v2.js map.js index.html style.css 2>/dev/null && {
        echo "🔄 Static file changed, copying..."
        cp three3d_v2.js map.js index.html style.css dist/
        echo "✅ Files updated!"
    }
done
