# Diagonale : traverser chaque forêt au lieu de viser son centre (évite les allers-retours).
# usage: dg_cross.py legs — entrée côté forêt précédente, sortie côté suivante, rentrées de
#                           IN m dans la forêt. Sortie : fr/dg/legs.json, fr/dg/polys.json
#        dg_cross.py trim — coupe dans fr/dg/leg_XX.json les allers-retours restants aux
#                           jonctions : demi-tour ramené au premier point dans la forêt.
import json, glob, math, sys
from collections import defaultdict
D, IN, TOL = "fr/dg", 200, 30

def km(a, b):
    la1, lo1, la2, lo2 = map(math.radians, (a[0], a[1], b[0], b[1]))
    h = math.sin((la2 - la1) / 2) ** 2 + math.cos(la1) * math.cos(la2) * math.sin((lo2 - lo1) / 2) ** 2
    return 2 * 6371 * math.asin(math.sqrt(h))

def inside(r, x, y):
    c = False
    for (x1, y1), (x2, y2) in zip(r, r[1:]):
        if (y1 > y) != (y2 > y) and x < x1 + (y - y1) * (x2 - x1) / (y2 - y1): c = not c
    return c

def in_forest(rings, lat, lon): return any(inside(r, lon, lat) for r in rings)

def rings_of(names):
    raw = defaultdict(list)
    for p in sorted(glob.glob("fr/dom_*.json") + glob.glob("fr/pub_*.json")):
        for f in json.load(open(p))["features"]:
            g, n = f["geometry"], f["properties"]["toponyme"]
            if g and n in names:
                raw[n] += [poly[0] for poly in ([g["coordinates"]] if g["type"] == "Polygon" else g["coordinates"])]
    return raw

def group_near(rings, lat, lon):
    # Parcelles à moins de ~2 km de celle qui contient (ou approche) le centre, comme fr_forests_index.
    bb = [(min(p[0] for p in r), min(p[1] for p in r), max(p[0] for p in r), max(p[1] for p in r)) for r in rings]
    seed = next((i for i, r in enumerate(rings) if inside(r, lon, lat)),
                min(range(len(rings)), key=lambda i: min(km((lat, lon), (p[1], p[0])) for p in rings[i])))
    keep, todo = {seed}, [seed]
    while todo:
        a = bb[todo.pop()]
        for j, b in enumerate(bb):
            if j not in keep and a[0]-.027 <= b[2] and b[0]-.027 <= a[2] and a[1]-.018 <= b[3] and b[1]-.018 <= a[3]:
                keep.add(j); todo.append(j)
    return [rings[i] for i in sorted(keep)]

def gate(rings, centre, toward):
    # Point du contour le plus proche de `toward`, rentré de IN m vers le centre de la forêt.
    lon, lat = min((p for r in rings for p in r), key=lambda p: km((p[1], p[0]), toward))
    d = km((lat, lon), centre) * 1000
    for step in (IN, IN / 2, 0):
        t = min(1, step / d) if d else 0
        q = (lat + (centre[0] - lat) * t, lon + (centre[1] - lon) * t)
        if step == 0 or in_forest(rings, *q): return {"lat": round(q[0], 6), "lon": round(q[1], 6)}

def legs():
    F = json.load(open("fr/diagonale.json"))
    short = lambda n: n.replace("Forêt Domaniale ", "forêt ").replace("Forêt Communale ", "forêt communale ")
    R = rings_of({f["name"] for f in F})
    polys, gates = [], []
    for k, f in enumerate(F):
        c = (f["lat"], f["lon"]); rings = group_near(R[f["name"]], *c); polys.append(rings)
        prev = (F[k-1]["lat"], F[k-1]["lon"]) if k else None
        nxt = (F[k+1]["lat"], F[k+1]["lon"]) if k + 1 < len(F) else None
        centre = {"lat": f["lat"], "lon": f["lon"]}
        gates.append((gate(rings, c, prev) if prev else centre, gate(rings, c, nxt) if nxt else centre))
    out = []
    for k, f in enumerate(F):
        (e, s), n = gates[k], short(f["name"])
        if 0 < k < len(F) - 1:
            out.append({"region": "Diagonale des forêts isolées", "start": e, "end": s,
                        "from": n + " (entrée)", "to": n + " (sortie)", "forest": k})
            print(f"{n[:45]:45} traversée {km((e['lat'], e['lon']), (s['lat'], s['lon'])):5.1f} km vol d'oiseau")
        if k + 1 < len(F):
            m = short(F[k+1]["name"]); last = k + 2 == len(F)
            out.append({"region": "Diagonale des forêts isolées", "start": s, "end": gates[k+1][0],
                        "from": n + ("" if k == 0 else " (sortie)"), "to": m + ("" if last else " (entrée)"),
                        "forest": k + 1})
    json.dump(out, open(f"{D}/legs.json", "w"), ensure_ascii=False, indent=0)
    json.dump(polys, open(f"{D}/polys.json", "w"))
    print(len(out), "étapes")

ARRAYS = ("path", "point_surfaces", "point_ign")

def cut(j, lo, hi):
    n = len(j["path"])
    for k in ARRAYS:
        if isinstance(j.get(k), list) and len(j[k]) == n: j[k] = j[k][lo:hi]
    el = j["elevation_profile"]["elevations"]
    if len(el) == n: j["elevation_profile"]["elevations"] = el[lo:hi]
    p = [(q["lat"], q["lon"]) for q in j["path"]]
    j["distance_km"] = sum(km(a, b) for a, b in zip(p, p[1:]))

def trim():
    L, polys = json.load(open(f"{D}/legs.json")), json.load(open(f"{D}/polys.json"))
    J = [json.load(open(f"{D}/leg_{i:02d}.json")) for i in range(len(L))]
    for i in range(len(L) - 1):
        a_, b_ = J[i], J[i+1]
        A = [(q["lat"], q["lon"]) for q in a_["path"]]; B = [(q["lat"], q["lon"]) for q in b_["path"]]
        a, b = len(A) - 1, 0
        while a > 0 and b < len(B) - 1:
            m = min(km(A[a-1], B[b+1]), km(A[a-1], B[b]), km(A[a], B[b+1])) * 1000
            if m > TOL: break
            if m == km(A[a-1], B[b+1]) * 1000: a -= 1; b += 1
            elif m == km(A[a-1], B[b]) * 1000: a -= 1
            else: b += 1
        if a == len(A) - 1: continue
        rings = polys[L[i]["forest"]]
        a2 = next((x for x in range(a, len(A)) if in_forest(rings, *A[x])), len(A) - 1)
        b2 = min(range(b + 1), key=lambda y: km(B[y], A[a2]))
        before = sum(km(p, q) for p, q in zip(A[a:], A[a+1:]))
        after = sum(km(p, q) for p, q in zip(A[a:a2+1], A[a+1:a2+1]))
        cut(a_, 0, a2 + 1); cut(b_, b2, len(B))
        print(f"jonction {i:02d}/{i+1:02d} {L[i]['to'][:40]:40} aller-retour {before:5.2f} → {after:5.2f} km")
    for i, j in enumerate(J): json.dump(j, open(f"{D}/leg_{i:02d}.json", "w"))

{"legs": legs, "trim": trim}[sys.argv[1]]()
