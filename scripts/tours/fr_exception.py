# Boucle des 15 Forêts d'Exception (ONF), liaisons enrichies de forêts publiques
# relais : à moins de LAT km de la ligne, une tous les ~40 km, la plus grande.
# Sortie : fr/exception.json [{name, lat, lon, ha, exception}]
import json, math
I = json.load(open("fr/index.json"))
EXC = ["Forêt Domaniale de Fontainebleau", "Forêt Domaniale de Roumare", "Forêt Domaniale de Bercé",
       "Forêt Domaniale de Tronçais", "Forêt Domaniale de la-Teste", "Forêt Domaniale de l'Aigoual",
       "Forêt Domaniale de la Sainte-Baume", "Forêt Domaniale de Boscodon", "Forêt Domaniale de la-Grande-Chartreuse",
       "Forêt Domaniale du Val-Suzon", "Forêt Domaniale de Darney", "Forêt Domaniale Indivise de Haguenau",
       "Forêt Domaniale de Verdun", "Forêt Domaniale de Verzy", "Forêt Domaniale de Retz"]
anchors = [next(f for f in I if f["name"] == n) for n in EXC]
LAT, MIN_HA, STEP, WIN = 20, 800, 40, (25, 55)
K = lambda lat: (111.32 * math.cos(math.radians(lat)), 110.54)
def frame(a, b):
    kx, ky = K((a["lat"] + b["lat"]) / 2)
    dx, dy = (b["lon"] - a["lon"]) * kx, (b["lat"] - a["lat"]) * ky
    D = math.hypot(dx, dy); ux, uy = dx / D, dy / D
    def proj(f):
        px, py = (f["lon"] - a["lon"]) * kx, (f["lat"] - a["lat"]) * ky
        return px * ux + py * uy, abs(-px * uy + py * ux)
    return D, proj
cands = [f for f in I if f["ha"] >= 400]
tour, total = [], 0
for i, a in enumerate(anchors):
    b = anchors[(i + 1) % len(anchors)]
    D, proj = frame(a, b)
    near = [(t, l, f) for f in cands for t, l in [proj(f)] if 10 < t < D - 10 and l <= 40 and f not in anchors]
    stops, cur = [], 0.0
    while D - cur > WIN[1]:
        # du plus exigeant au plus large : proche et grande, puis plus loin ou plus petite
        for lo, hi, lat, ha in ((*WIN, LAT, MIN_HA), (15, 70, LAT, MIN_HA), (15, 70, 40, MIN_HA), (15, 70, 40, 400)):
            w = [(t, l, f) for t, l, f in near if cur + lo <= t <= cur + hi and l <= lat and f["ha"] >= ha]
            if w: break
        if not w: cur += STEP; continue
        t, l, f = max(w, key=lambda x: x[2]["ha"] * (1.5 if "Domaniale" in x[2]["name"] else 1) / (1 + x[1] / 10))
        stops.append(f); cur = t
    tour.append({**a, "exception": True}); tour += [{**f, "exception": False} for f in stops]
    pts = [a] + stops + [b]
    seg = [frame(p, q)[0] for p, q in zip(pts, pts[1:])]; total += sum(seg)
    short = lambda n: n.replace("Forêt Domaniale ", "FD ").replace("Forêt Communale ", "FC ").replace("Indivise ", "")
    print(f"\n★ {short(a['name'])} → {short(b['name'])}  ({D:.0f} km, plus long saut {max(seg):.0f} km)")
    for f in stops: print(f"    · {short(f['name'])}  {f['ha']} ha")
print(f"\n{len(tour)} forêts dont {len(anchors)} d'exception ; boucle {total:.0f} km vol d'oiseau")
json.dump(tour, open("fr/exception.json", "w"), ensure_ascii=False, indent=0)
