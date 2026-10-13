# usage: leg_via.py DIR idx W lat,lon — étape idx via un point, fusionne en DIR/leg_XX_wW.json
import json, sys, urllib.request
D, i, W, via = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
F = json.load(open(f"{D}/forests.json")); o = json.load(open(f"{D}/order.json"))
a, b = F[o[i]], F[o[(i+1) % len(o)]]; vl, vo = map(float, via.split(","))
pts = [(a["lat"], a["lon"]), (vl, vo), (b["lat"], b["lon"])]
parts = []
for (l1, o1), (l2, o2) in zip(pts, pts[1:]):
    body = json.dumps({"start": {"lat": l1, "lon": o1}, "end": {"lat": l2, "lon": o2}, "w_pop": 1, "w_paved": 1, "w_forest": W}).encode()
    req = urllib.request.Request("http://localhost:8090/api/route", body, {"Content-Type": "application/json"})
    parts.append(json.load(urllib.request.urlopen(req, timeout=3500)))
r = parts[0]
for p in parts[1:]:
    r["path"] += p["path"][1:]; r["point_surfaces"] += p["point_surfaces"][1:]; r["point_ign"] += p["point_ign"][1:]
    r["elevation_profile"]["elevations"] += p["elevation_profile"]["elevations"][1:]; r["distance_km"] += p["distance_km"]
json.dump(r, open(f"{D}/leg_{i:02d}_w{W}.json", "w"))
print(f"leg {i} w{W} via {via} : {r['distance_km']:.1f} km")
