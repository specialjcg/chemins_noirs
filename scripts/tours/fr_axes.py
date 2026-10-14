# Axe le plus long de chaque forêt domaniale (BD TOPO foret_publique).
# Sortie : fr/axes.json, triée par longueur d'axe.
import json, math, glob, sys
from collections import defaultdict

feats = []
for f in sorted(glob.glob("dom_*.json")):
    feats += json.load(open(f))["features"]

first = feats[0]["geometry"]["coordinates"]
while isinstance(first[0], list): first = first[0]
swap = first[0] > 40  # EPSG:4326 en WFS 2.0 : lat, lon
def outer_rings(g):
    polys = [g["coordinates"]] if g["type"] == "Polygon" else g["coordinates"]
    return [[(p[1], p[0]) if swap else (p[0], p[1]) for p in poly[0]] for poly in polys]

# Métropole et Corse seulement ; une forêt = parcelles de même nom à moins
# de GAP km les unes des autres (les homonymes dispersés restent séparés).
GAP = 2.0
raw = defaultdict(list)
for f in feats:
    if not f["geometry"]: continue
    for r in outer_rings(f["geometry"]):
        x, y = r[0]
        if -5.5 < x < 10 and 41 < y < 51.5: raw[f["properties"]["toponyme"] or "?"].append(r)
by_name = {}
for name, rings in raw.items():
    bb = [(min(p[0] for p in r), min(p[1] for p in r), max(p[0] for p in r), max(p[1] for p in r)) for r in rings]
    parent = list(range(len(rings)))
    def find(i):
        while parent[i] != i: parent[i] = parent[parent[i]]; i = parent[i]
        return i
    dx, dy = GAP / 75, GAP / 111
    for i in range(len(rings)):
        for j in range(i):
            a, b = bb[i], bb[j]
            if a[0]-dx <= b[2] and b[0]-dx <= a[2] and a[1]-dy <= b[3] and b[1]-dy <= a[3]: parent[find(i)] = find(j)
    groups = defaultdict(list)
    for i, r in enumerate(rings): groups[find(i)].append(r)
    gs = sorted(groups.values(), key=lambda g: -sum(len(r) for r in g))
    for k, g in enumerate(gs): by_name[name if k == 0 else f"{name} #{k+1}"] = g

def hull(pts):
    pts = sorted(set(pts))
    if len(pts) < 3: return pts
    def cross(o, a, b): return (a[0]-o[0])*(b[1]-o[1]) - (a[1]-o[1])*(b[0]-o[0])
    lo, up = [], []
    for p in pts:
        while len(lo) >= 2 and cross(lo[-2], lo[-1], p) <= 0: lo.pop()
        lo.append(p)
    for p in reversed(pts):
        while len(up) >= 2 and cross(up[-2], up[-1], p) <= 0: up.pop()
        up.append(p)
    return lo[:-1] + up[:-1]

def inside(ring, x, y):
    c = False
    for (x1, y1), (x2, y2) in zip(ring, ring[1:]):
        if (y1 > y) != (y2 > y) and x < x1 + (y - y1) * (x2 - x1) / (y2 - y1): c = not c
    return c

out = []
for name, rings in by_name.items():
    lat0 = sum(p[1] for r in rings for p in r) / sum(len(r) for r in rings)
    kx, ky = 111.32 * math.cos(math.radians(lat0)), 110.54
    h = hull([p for r in rings for p in r])
    best = (0, None, None)
    for i, a in enumerate(h):
        for b in h[i+1:]:
            d = math.hypot((a[0]-b[0])*kx, (a[1]-b[1])*ky)
            if d > best[0]: best = (d, a, b)
    d, a, b = best
    if d < 15: out.append({"name": name, "axis_km": round(d, 1)}); continue
    boxes = [(min(p[0] for p in r), min(p[1] for p in r), max(p[0] for p in r), max(p[1] for p in r), r) for r in rings]
    n, hit = 200, 0
    for k in range(n + 1):
        x, y = a[0] + (b[0]-a[0]) * k / n, a[1] + (b[1]-a[1]) * k / n
        if any(x0 <= x <= x1 and y0 <= y <= y1 and inside(r, x, y) for x0, y0, x1, y1, r in boxes): hit += 1
    area = sum(abs(sum(p[0]*q[1]-q[0]*p[1] for p, q in zip(r, r[1:])))/2*kx*ky for r in rings) * 100
    out.append({"name": name, "axis_km": round(d, 1), "inside": round(hit / (n + 1), 2),
                "area_ha": round(area), "a": [round(a[1], 5), round(a[0], 5)], "b": [round(b[1], 5), round(b[0], 5)]})
out.sort(key=lambda f: -f["axis_km"])
json.dump(out, open("axes.json", "w"), ensure_ascii=False, indent=0)
big = [f for f in out if f["axis_km"] >= 30]
for t in (15, 20, 25, 30): print(t, "km :", sum(f["axis_km"] >= t for f in out), "dont dedans>=50%:", sum(f["axis_km"] >= t and f.get("inside",0) >= .5 for f in out))
print(f"{len(feats)} entités, {len(out)} forêts, {len(big)} ≥ 30 km, swap={swap}")
for f in big: print(f'{f["axis_km"]:6.1f} km  {f["inside"]*100:3.0f}% dedans  {f["area_ha"]:7d} ha  {f["name"]}')
