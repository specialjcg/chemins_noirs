#!/usr/bin/env bash
# Test simple pour vérifier si les waypoints OSM sont utilisés
set -euo pipefail

cd "$(dirname "$0")"

echo "🧪 Test Simple des Waypoints OSM"
echo "================================"
echo ""

# Coordonnées de test proches (même bbox que la route sauvegardée)
START_LAT=45.9306
START_LON=4.5778
END_LAT=45.9334
END_LON=4.5783

echo "📍 Test avec 2 points proches:"
echo "   Départ: $START_LAT, $START_LON"
echo "   Arrivée: $END_LAT, $END_LON"
echo ""
echo "⏱️  Envoi de la requête au backend (port 8080)..."
echo ""

# Requête HTTP au backend
RESPONSE=$(curl -s -w "\n%{http_code}" -X POST http://localhost:8080/api/route \
  -H "Content-Type: application/json" \
  -d "{
    \"start\": {\"lat\": $START_LAT, \"lon\": $START_LON},
    \"end\": {\"lat\": $END_LAT, \"lon\": $END_LON}
  }")

HTTP_CODE=$(echo "$RESPONSE" | tail -1)
BODY=$(echo "$RESPONSE" | head -n -1)

if [ "$HTTP_CODE" != "200" ]; then
    echo "❌ Erreur HTTP $HTTP_CODE"
    echo "$BODY"
    exit 1
fi

echo "✅ Réponse reçue (HTTP 200)"
echo ""

# Analyser le JSON pour compter les coordonnées
POINT_COUNT=$(echo "$BODY" | jq -r '.path | length')
DISTANCE_KM=$(echo "$BODY" | jq -r '.distanceKm')

echo "📊 Résultats:"
echo "   Distance: ${DISTANCE_KM} km"
echo "   Points dans le path: $POINT_COUNT"
echo ""

# Interprétation
if [ "$POINT_COUNT" -le 2 ]; then
    echo "❌ PROBLÈME: Seulement $POINT_COUNT points (ligne droite)"
    echo "   Les waypoints OSM ne sont PAS utilisés"
elif [ "$POINT_COUNT" -le 10 ]; then
    echo "⚠️  ATTENTION: $POINT_COUNT points (peut-être quelques waypoints)"
    echo "   Vérifiez visuellement si la route suit les chemins OSM"
else
    echo "✅ SUCCÈS: $POINT_COUNT points (waypoints OSM utilisés!)"
    echo "   La route devrait suivre les chemins OSM"
fi

echo ""
echo "💡 Pour voir les logs détaillés du backend:"
echo "   Regardez la console où tourne ./target/release/backend_partial"
echo "   Vous devriez voir: 'expand_path_with_waypoints: ... added X waypoints'"
echo ""
echo "🌐 Pour tester visuellement:"
echo "   Ouvrez http://localhost:8081 (ou le port du frontend)"
echo "   Créez une route avec ces mêmes coordonnées"
