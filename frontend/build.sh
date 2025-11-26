#!/bin/bash
# Build script for wasm-pack (replaces Trunk)

set -euo pipefail

echo "🔨 Building frontend with wasm-pack..."

# Build with wasm-pack (skip wasm-opt for now to debug)
rm -rf pkg
WASM_PACK_PROFILE=dev wasm-pack build --target web --out-dir pkg --no-typescript --dev

# Install MapLibre GL v5.x
echo "📦 Installing MapLibre GL v5.x..."
npm install --no-save maplibre-gl@^5.0.0

echo "📦 Creating dist directory..."
rm -rf dist
mkdir -p dist

# Copy WASM + JS glue as generated (imports keep the wasm-bindgen snippet paths)
cp pkg/*.wasm dist/
cp pkg/frontend.js dist/
cp pkg/frontend_bg.js dist/ 2>/dev/null || true

# Copy static assets
cp style.css maplibre_map.js dist/

# Copy MapLibre GL v5.x from node_modules
echo "📦 Copying MapLibre GL v5.x..."
mkdir -p dist/maplibre-gl
cp -r node_modules/maplibre-gl/dist/* dist/maplibre-gl/

# Copy the wasm-bindgen snippets (maplibre_map.js) with the generated hash
SNIPPET_DIR="$(find pkg/snippets -maxdepth 1 -mindepth 1 -type d -printf '%f\n' | head -n 1)"
if [[ -z "$SNIPPET_DIR" ]]; then
  echo "❌ Could not locate pkg/snippets directory produced by wasm-pack."
  exit 1
fi
mkdir -p "dist/snippets"
cp -r "pkg/snippets/${SNIPPET_DIR}" "dist/snippets/"
# Ensure the latest JS helpers are also mirrored (in case they changed without rebuilding wasm-bindgen snippets)
cp maplibre_map.js "dist/snippets/${SNIPPET_DIR}/"

# Create a simple index.html that loads the WASM
cat > dist/index.html << 'EOF'
<!DOCTYPE html>
<html lang="fr">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Chemins Noirs</title>
    <link rel="stylesheet" href="style.css" />
    <link
      rel="stylesheet"
      href="https://unpkg.com/maplibre-gl@5.13.0/dist/maplibre-gl.css"
      crossorigin=""
    />
    <script type="importmap">
      {
        "imports": {
          "maplibre-gl": "https://cdn.jsdelivr.net/npm/maplibre-gl@5.13.0/+esm"
        }
      }
    </script>
  </head>
  <body>
    <div id="app"></div>
    <div id="map"></div>

    <script type="module">
      import init from './frontend.js';

      async function run() {
        await init();
      }

      run();
    </script>

    <script type="module" src="maplibre_map.js"></script>
  </body>
</html>
EOF

echo "✅ Build complete! Files in dist/"
echo "🚀 Run 'python3 -m http.server 8081 --directory dist' to serve"
