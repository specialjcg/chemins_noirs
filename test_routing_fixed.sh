#!/usr/bin/env bash
# Test script avec coordonnées valides dans Lyon

echo "======================================"
echo "Test de Routage - Coordonnées Lyon"
echo "======================================"
echo ""

# Test 1: Route simple dans Lyon
echo "Test 1: Route Place Bellecour → Fourvière"
echo "-----------------------------------------"
time curl -X POST http://localhost:8080/api/route/multi \
  -H "Content-Type: application/json" \
  -d '{
    "waypoints": [
      {"lat": 45.760, "lon": 4.835},
      {"lat": 45.770, "lon": 4.825}
    ],
    "w_pop": 1.5,
    "w_paved": 4.0,
    "close_loop": false
  }' 2>&1 | tee /tmp/test_route_lyon.json

echo ""
echo ""

# Analyser le résultat
if grep -q "\"path\":" /tmp/test_route_lyon.json; then
    WAYPOINT_COUNT=$(jq '.path | length' /tmp/test_route_lyon.json 2>/dev/null || echo "0")
    DISTANCE=$(jq '.distance_km' /tmp/test_route_lyon.json 2>/dev/null || echo "0")

    echo "✅ Route générée avec succès!"
    echo "   - Waypoints: $WAYPOINT_COUNT"
    echo "   - Distance: ${DISTANCE}km"

    if [ "$WAYPOINT_COUNT" -lt 10 ]; then
        echo "   ⚠️  PROBLÈME: Trop peu de waypoints (lignes droites?)"
    elif [ "$WAYPOINT_COUNT" -gt 100 ]; then
        echo "   ✅ Bonne densité de waypoints"
    fi
else
    echo "❌ Échec du routage"
    grep -i "error\|not found" /tmp/test_route_lyon.json || echo "   (voir /tmp/test_route_lyon.json)"
fi

echo ""
echo "Fichier de résultat: /tmp/test_route_lyon.json"
echo "Diagnostic complet: ./DIAGNOSTIC_ROUTING_ISSUES.md"
