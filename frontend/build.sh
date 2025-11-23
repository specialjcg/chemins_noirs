#!/bin/bash
# Build script for wasm-pack (replaces Trunk)

set -euo pipefail

echo "🔨 Building frontend with wasm-pack..."

# Build with wasm-pack
wasm-pack build --target web --out-dir pkg --no-typescript

echo "📦 Creating dist directory..."
rm -rf dist
mkdir -p dist

# Copy WASM + JS glue as generated (imports keep the wasm-bindgen snippet paths)
cp pkg/*.wasm dist/
cp pkg/frontend.js dist/
cp pkg/frontend_bg.js dist/ 2>/dev/null || true

# Copy static assets
cp index.html style.css map.js three3d_clean.js dist/

# Copy the wasm-bindgen snippets (map.js / three3d_clean.js) with the generated hash
SNIPPET_DIR="$(find pkg/snippets -maxdepth 1 -mindepth 1 -type d -printf '%f\n' | head -n 1)"
if [[ -z "$SNIPPET_DIR" ]]; then
  echo "❌ Could not locate pkg/snippets directory produced by wasm-pack."
  exit 1
fi
mkdir -p "dist/snippets"
cp -r "pkg/snippets/${SNIPPET_DIR}" "dist/snippets/"
# Ensure the latest JS helpers are also mirrored (in case they changed without rebuilding wasm-bindgen snippets)
cp map.js three3d_clean.js "dist/snippets/${SNIPPET_DIR}/"

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
      href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css"
      crossorigin=""
    />
    <script
      src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"
      crossorigin=""
    ></script>
    <script type="importmap">
      {
        "imports": {
          "three": "https://cdn.jsdelivr.net/npm/three@0.160.0/build/three.module.js"
        }
      }
    </script>
  </head>
  <body>
    <div id="app"></div>
    <div id="map"></div>
    <div id="three3dContainer"></div>

    <script type="module">
      import init from './frontend.js';

      async function run() {
        await init();
      }

      run();
    </script>

    <script type="module" src="map.js"></script>
    <script type="module" src="three3d_clean.js"></script>
  </body>
</html>
EOF

echo "✅ Build complete! Files in dist/"
echo "🚀 Run 'python3 -m http.server 8081 --directory dist' to serve"
