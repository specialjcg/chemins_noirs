# Chaînes de forêts domaniales : forêts à moins de GAP km les unes des autres,
# dont l'étendue dépasse MIN km. usage: fr_chains.py [GAP] [MIN]
import json, glob, math, sys
from collections import defaultdict
GAP = float(sys.argv[1]) if len(sys.argv) > 1 else 3
MIN = float(sys.argv[2]) if len(sys.argv) > 2 else 100
import os
FILES = os.environ.get("FILES", "fr/dom_*.json")
feats = [f for p in sorted(q for pat in FILES.split() for q in glob.glob(pat)) for f in json.load(open(p))["features"] if f["geometry"]]
def outer(g):
    return [poly[0] for poly in ([g["coordinates"]] if g["type"] == "Polygon" else g["coordinates"])]
# une entrée par parcelle de métropole hors Corse : (nom, sommets échantillonnés en km)
K = 111.32 * math.cos(math.radians(46.5)), 110.54
parts = []
for f in feats:
    for r in outer(f["geometry"]):
        x, y = r[0]
        if not (-5.5 < x < 10 and 41 < y < 51.5) or (x > 8.4 and y < 43.1): continue
        step = max(1, len(r) // 60)
        pts = [(p[0] * K[0], p[1] * K[1]) for p in r[::step]]
        parts.append((f["properties"]["toponyme"], pts,
                      (min(p[0] for p in pts), min(p[1] for p in pts), max(p[0] for p in pts), max(p[1] for p in pts))))
# grille pour ne comparer que les parcelles voisines
cell = defaultdict(list)
for i, (_, _, (x0, y0, x1, y1)) in enumerate(parts):
    for cx in range(int(x0 // 20), int(x1 // 20) + 1):
        for cy in range(int(y0 // 20), int(y1 // 20) + 1): cell[cx, cy].append(i)
parent = list(range(len(parts)))
def find(i):
    while parent[i] != i: parent[i] = parent[parent[i]]; i = parent[i]
    return i
seen = set()
for ids in cell.values():
    for a in ids:
        for b in ids:
            if b <= a or (a, b) in seen: continue
            seen.add((a, b))
            A, B = parts[a][2], parts[b][2]
            if A[0] - GAP > B[2] or B[0] - GAP > A[2] or A[1] - GAP > B[3] or B[1] - GAP > A[3]: continue
            if find(a) == find(b): continue
            if any(math.hypot(p[0]-q[0], p[1]-q[1]) < GAP for p in parts[a][1] for q in parts[b][1]):
                parent[find(a)] = find(b)
def hull(pts):
    pts = sorted(set(pts))
    if len(pts) < 3: return pts
    cross = lambda o, a, b: (a[0]-o[0])*(b[1]-o[1]) - (a[1]-o[1])*(b[0]-o[0])
    lo, up = [], []
    for p in pts:
        while len(lo) >= 2 and cross(lo[-2], lo[-1], p) <= 0: lo.pop()
        lo.append(p)
    for p in reversed(pts):
        while len(up) >= 2 and cross(up[-2], up[-1], p) <= 0: up.pop()
        up.append(p)
    return lo[:-1] + up[:-1]
groups = defaultdict(list)
for i in range(len(parts)): groups[find(i)].append(i)
out = []
for ids in groups.values():
    if len(ids) < 2: continue
    # étendue : plus grande distance entre deux sommets de l'enveloppe convexe
    h = hull([p for i in ids for p in parts[i][1]])
    ext = max(((p, q) for p in h for q in h), key=lambda pq: math.hypot(pq[0][0]-pq[1][0], pq[0][1]-pq[1][1]))
    d = math.hypot(ext[0][0]-ext[1][0], ext[0][1]-ext[1][1])
    if d < MIN: continue
    # forêts dans l'ordre le long de l'axe de la chaîne
    ux, uy = (ext[1][0]-ext[0][0]) / d, (ext[1][1]-ext[0][1]) / d
    pos = defaultdict(list)
    for i in ids:
        for p in parts[i][1][::10]: pos[parts[i][0]].append((p[0]-ext[0][0]) * ux + (p[1]-ext[0][1]) * uy)
    names = sorted(pos, key=lambda n: sum(pos[n]) / len(pos[n]))
    lat = lambda p: p[1] / K[1]; lon = lambda p: p[0] / K[0]
    out.append((d, names, (lat(ext[0]), lon(ext[0])), (lat(ext[1]), lon(ext[1]))))
out.sort(key=lambda o: -o[0])
for d, names, a, b in out:
    dom = [n for n in names if "Domaniale" in n]
    print(f"\n{d:.0f} km  ({a[0]:.3f},{a[1]:.3f}) -> ({b[0]:.3f},{b[1]:.3f})  {len(names)} forêts dont {len(dom)} domaniales")
    # domaniales dans l'ordre, les autres comptées entre elles
    line, k = [], 0
    for n in names:
        if n in dom:
            if k: line.append(f"(+{k})")
            line.append(n.replace("Forêt Domaniale ", "").replace("Indivise ", "")); k = 0
        else: k += 1
    if k: line.append(f"(+{k})")
    print("  " + " → ".join(line))
