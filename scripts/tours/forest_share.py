import json, sys, math
import numpy as np
from matplotlib.path import Path
S = sys.argv[1]
polys = []
for f in json.load(open(f"{S}/forest.geojson"))["features"]:
    g = f["geometry"]
    for poly in ([g["coordinates"]] if g["type"] == "Polygon" else g["coordinates"]):
        outer = Path(poly[0]); holes = [Path(h) for h in poly[1:]]
        xs = [c[0] for c in poly[0]]; ys = [c[1] for c in poly[0]]
        polys.append((min(xs), min(ys), max(xs), max(ys), outer, holes))
def inside(pts):
    res = np.zeros(len(pts), bool)
    for a, b, c, d, outer, holes in polys:
        m = (pts[:,0] >= a) & (pts[:,0] <= c) & (pts[:,1] >= b) & (pts[:,1] <= d) & ~res
        if not m.any(): continue
        sub = pts[m]; hit = outer.contains_points(sub)
        for h in holes: hit &= ~h.contains_points(sub)
        res[np.where(m)[0][hit]] = True
    return res
def km(a, b): return math.hypot((a[1]-b[1])*111.2, (a[0]-b[0])*77.3)
def analyse(path, label):
    pts, cum = [], []
    t = 0.0
    for p, q in zip(path, path[1:]):
        L = km(p, q); n = max(1, int(L / 0.02))
        for i in range(n):
            pts.append((p[0] + (q[0]-p[0])*i/n, p[1] + (q[1]-p[1])*i/n)); cum.append(t + L*i/n)
        t += L
    pts = np.array(pts); ins = inside(pts)
    step = np.diff(cum, append=t)
    share = 100 * step[ins].sum() / t
    gaps, cur = [], None
    for i, v in enumerate(ins):
        if not v and cur is None: cur = i
        if v and cur is not None: gaps.append((cur, i)); cur = None
    if cur is not None: gaps.append((cur, len(ins)))
    gaps = [(cum[a], cum[b-1] - cum[a] + step[b-1], pts[a]) for a, b in gaps]
    big = sorted(gaps, key=lambda g: -g[1])[:6]
    print(f"{label}: {t:.1f} km, forêt {share:.0f} %, hors forêt {t*(1-share/100):.1f} km en {len(gaps)} tronçons")
    for at, L, p in big:
        print(f"   km {at:5.1f} : {L*1000:5.0f} m hors forêt  ({p[1]:.4f}, {p[0]:.4f})")
for name in sys.argv[2:]:
    j = json.load(open(f"{S}/{name}"))
    analyse([(c["lon"], c["lat"]) for c in j["path"]], name)
