# Index des forêts publiques de métropole : nom, centre (point intérieur), aire.
# Parcelles de même nom regroupées si à moins de 2 km. Sortie : fr/index.json
import json, glob, math
from collections import defaultdict
feats = [f for p in sorted(glob.glob("fr/dom_*.json") + glob.glob("fr/pub_*.json")) for f in json.load(open(p))["features"] if f["geometry"]]
def outer(g): return [poly[0] for poly in ([g["coordinates"]] if g["type"] == "Polygon" else g["coordinates"])]
def area(r, kx, ky): return abs(sum(p[0]*q[1]-q[0]*p[1] for p, q in zip(r, r[1:]))) / 2 * kx * ky
def inside(r, x, y):
    c = False
    for (x1, y1), (x2, y2) in zip(r, r[1:]):
        if (y1 > y) != (y2 > y) and x < x1 + (y - y1) * (x2 - x1) / (y2 - y1): c = not c
    return c
raw = defaultdict(list)
for f in feats:
    for r in outer(f["geometry"]):
        x, y = r[0]
        if -5.5 < x < 10 and 41 < y < 51.5 and not (x > 8.4 and y < 43.1):
            raw[(f["properties"]["toponyme"] or "?", f["properties"]["nature"])].append(r)
out = []
for (name, nature), rings in raw.items():
    bb = [(min(p[0] for p in r), min(p[1] for p in r), max(p[0] for p in r), max(p[1] for p in r)) for r in rings]
    parent = list(range(len(rings)))
    def find(i):
        while parent[i] != i: parent[i] = parent[parent[i]]; i = parent[i]
        return i
    for i in range(len(rings)):
        for j in range(i):
            a, b = bb[i], bb[j]
            if a[0]-.027 <= b[2] and b[0]-.027 <= a[2] and a[1]-.018 <= b[3] and b[1]-.018 <= a[3]: parent[find(i)] = find(j)
    groups = defaultdict(list)
    for i, r in enumerate(rings): groups[find(i)].append(r)
    for g in groups.values():
        lat0 = sum(p[1] for r in g for p in r) / sum(len(r) for r in g)
        kx, ky = 111.32 * math.cos(math.radians(lat0)), 110.54
        big = max(g, key=lambda r: area(r, kx, ky))
        cx = sum(p[0] for p in big) / len(big); cy = sum(p[1] for p in big) / len(big)
        if not inside(big, cx, cy):
            xs = sorted(x1+(cy-y1)*(x2-x1)/(y2-y1) for (x1, y1), (x2, y2) in zip(big, big[1:]) if (y1 > cy) != (y2 > cy))
            if len(xs) >= 2: cx = (xs[0] + xs[1]) / 2
        out.append({"name": name, "nature": nature, "lat": round(cy, 5), "lon": round(cx, 5),
                    "ha": round(sum(area(r, kx, ky) for r in g) * 100)})
out.sort(key=lambda f: -f["ha"])
json.dump(out, open("fr/index.json", "w"), ensure_ascii=False)
print(len(out), "forêts ;", sum(f["ha"] >= 1000 for f in out), ">= 1000 ha")
