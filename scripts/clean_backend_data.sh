#!/bin/bash

# Clean unused backend data files
# Frees up ~15 GB of disk space

set -e

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DATA_DIR="$ROOT_DIR/backend/data"

cd "$DATA_DIR"

echo "🧹 Backend Data Cleanup"
echo "======================"
echo ""

# Calculate total size to delete
TOTAL_SIZE=$(du -sh test_generated.json generated_graph.json RGEALTI_2-0_5M* tiles.INVALID rhone-alpes.osm.pbf dem/region.tif 2>/dev/null | awk '{sum+=$1} END {print sum}' || echo "0")

echo "Files to delete:"
echo ""
ls -lh test_generated.json generated_graph.json rhone-alpes.osm.pbf 2>/dev/null | awk '{print "  " $9 ": " $5}'
ls -lh RGEALTI_2-0_5M* 2>/dev/null | awk '{print "  " $9 ": " $5}'
ls -lh dem/region.tif 2>/dev/null | awk '{print "  " $9 ": " $5}'
du -sh tiles.INVALID 2>/dev/null | awk '{print "  tiles.INVALID: " $1}'
echo ""
echo "Total space to free: ~15 GB"
echo ""

read -p "⚠️  Delete these files? (y/N) " -n 1 -r
echo

if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo ""
    echo "Deleting files..."

    # Delete old test/generated files
    rm -f test_generated.json
    echo "  ✅ Deleted test_generated.json"

    rm -f generated_graph.json
    echo "  ✅ Deleted generated_graph.json"

    # Delete old DEM data
    rm -rf RGEALTI_2-0_5M*
    echo "  ✅ Deleted RGEALTI_2-0_5M*"

    rm -f dem/region.tif
    echo "  ✅ Deleted dem/region.tif"

    # Delete duplicate PBF
    rm -f rhone-alpes.osm.pbf
    echo "  ✅ Deleted rhone-alpes.osm.pbf (keeping rhone-alpes-251111.osm.pbf)"

    # Delete invalid tiles
    rm -rf tiles.INVALID
    echo "  ✅ Deleted tiles.INVALID/"

    echo ""
    echo "✅ Cleanup complete!"
    echo ""
    echo "Remaining data:"
    du -sh . 2>/dev/null
    echo ""
    echo "Kept files:"
    echo "  ✅ rhone-alpes-251111.osm.pbf (483 MB) - Current PBF"
    echo "  ✅ region.asc (3.7 GB) - DEM data"
    echo "  ✅ cache/ (14 MB) - Graph caches"
    echo "  ✅ saved_routes/ (76 KB) - PostgreSQL routes"
else
    echo "❌ Cancelled"
fi
