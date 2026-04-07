#!/usr/bin/env bash
# Debug script to test waypoints with coordinates from connected edges

set -euo pipefail
cd "$(dirname "$0")"

echo "=== Waypoints Debug Test ===="
echo ""

# Extract two nodes from the SAME edge (guaranteed to be connected)
echo "1. Extracting coordinates from a single edge..."
COORDS=$(zstd -d < backend/data/cache/148f9506f254916f.json.zst 2>/dev/null | \
  jq -r '.edges[1000] as $edge | 
    .nodes[] | select(.id == $edge.from or .id == $edge.to) | 
    {id, lat, lon}' 2>/dev/null | \
  jq -s '.')

START_LAT=$(echo "$COORDS" | jq -r '.[0].lat')
START_LON=$(echo "$COORDS" | jq -r '.[0].lon')
END_LAT=$(echo "$COORDS" | jq -r '.[1].lat')
END_LON=$(echo "$COORDS" | jq -r '.[1].lon')

echo "   Start node: lat=$START_LAT, lon=$START_LON"
echo "   End node: lat=$END_LAT, lon=$END_LON"
echo ""

echo "2. Sending route request..."
RESULT=$(curl -s -X POST http://localhost:8080/api/route \
  -H "Content-Type: application/json" \
  -d "{
    \"start\": {\"lat\": $START_LAT, \"lon\": $START_LON},
    \"end\": {\"lat\": $END_LAT, \"lon\": $END_LON}
  }")

echo "$RESULT" | jq . 2>/dev/null || echo "$RESULT"
echo ""

if echo "$RESULT" | jq -e '.path' > /dev/null 2>&1; then
  WAYPOINTS=$(echo "$RESULT" | jq '.path | length')
  DISTANCE=$(echo "$RESULT" | jq '.distance_km')
  echo "✅ SUCCESS! Route found:"
  echo "   - $WAYPOINTS waypoints"
  echo "   - ${DISTANCE}km distance"
  
  # Check backend logs for waypoint expansion
  echo ""
  echo "3. Checking backend logs for waypoint expansion..."
  tail -100 /tmp/backend_waypoint_test.log 2>/dev/null | \
    grep "expand_path_with_waypoints" | tail -1
else
  echo "❌ FAILED: No route found"
  echo ""
  echo "Backend logs:"
  tail -20 /tmp/backend_waypoint_test.log 2>/dev/null | tail -10
fi
