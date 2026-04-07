#!/usr/bin/env bash
# Script pour régénérer les tiles pré-calculées et accélérer le pathfinding

set -euo pipefail
cd "$(dirname "$0")"

echo "=== Génération des Tiles Pré-calculées ===" 
echo ""
echo "Ceci va pré-calculer des graphes pour différentes zones géographiques."
echo "Les prochaines requêtes de routing seront BEAUCOUP plus rapides (secondes au lieu de minutes)."
echo ""

# Check if PBF file exists
if [[ ! -f "backend/data/rhone-alpes-251111.osm.pbf" ]]; then
    echo "❌ Fichier PBF non trouvé: backend/data/rhone-alpes-251111.osm.pbf"
    exit 1
fi

echo "✅ Fichier PBF trouvé: backend/data/rhone-alpes-251111.osm.pbf"
echo ""

# Create tiles directory
mkdir -p backend/data/tiles
echo "✅ Répertoire tiles créé: backend/data/tiles"
echo ""

# Build the tile generator
echo "🔨 Compilation du générateur de tiles..."
cd backend
cargo build --release --bin generate_tiles
echo "✅ Générateur compilé"
echo ""

# Generate tiles
echo "🗺️  Génération des tiles (ceci peut prendre 10-30 minutes)..."
echo "   Les graphes pré-calculés seront stockés dans data/tiles/"
echo ""

export DATABASE_URL="${DATABASE_URL:-postgresql://chemins_user:vaccances1968@localhost/chemins_noirs}"
RUST_LOG=info ../target/release/generate_tiles

echo ""
echo "✅ Tiles générées avec succès!"
echo ""
echo "📊 Statistiques:"
ls -lh data/tiles/ | wc -l | xargs echo "   Nombre de tiles:"
du -sh data/tiles/ | awk '{print "   Taille totale: " $1}'
echo ""
echo "🚀 Redémarrez le backend pour utiliser les tiles:"
echo "   ./scripts/run_fullstack_elm.sh"
