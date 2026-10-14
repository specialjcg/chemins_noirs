# Diagonale des forêts isolées : piliers fixés, relais choisis tous les ~35 km
# parmi les forêts publiques >= 500 ha à moins de LAT km de la ligne, par score
# d'isolement (fr_remote.py). Sortie : fr/diagonale.json
import json, math
R = json.load(open("fr/remote500.json"))
PILIERS = ["Forêt Domaniale du Haut-Verdon", "Forêt Domaniale de Jocou", "Forêt Domaniale de Bonnefoi",
           "Forêt Domaniale de Mazan", "Forêt Domaniale du Bougès", "Forêt Domaniale d'Aubrac",
           "Forêt Domaniale de la Croix-de-Bor", "Forêt Communale de Clavières", "Forêt Domaniale d'Assise",
           "Forêt Domaniale de Buremont", "Forêt Domaniale de Saint-Prix", "Forêt Domaniale d'Auberive",
           "Forêt Domaniale de Montiers"]
anchors = [next(f for f in R if f["name"] == n) for n in PILIERS]
LAT, STEP, WIN = 25, 35, (20, 50)
def frame(a, b):
    kx, ky = 111.32 * math.cos(math.radians((a["lat"] + b["lat"]) / 2)), 110.54
    dx, dy = (b["lon"] - a["lon"]) * kx, (b["lat"] - a["lat"]) * ky
    D = math.hypot(dx, dy); ux, uy = dx / D, dy / D
    def proj(f):
        px, py = (f["lon"] - a["lon"]) * kx, (f["lat"] - a["lat"]) * ky
        return px * ux + py * uy, abs(-px * uy + py * ux)
    return D, proj
out, total, short = [], 0, lambda n: n.replace("Forêt Domaniale ", "FD ").replace("Forêt Communale ", "FC ").replace("Forêt Syndicale ", "FS ")
for a, b in zip(anchors, anchors[1:]):
    D, proj = frame(a, b)
    near = [(t, l, f) for f in R for t, l in [proj(f)] if 8 < t < D - 8 and l <= LAT and f["dens"] <= 30 and f["name"] not in PILIERS]
    stops, cur = [], 0.0
    while D - cur > WIN[1]:
        for lo, hi in (WIN, (12, 65)):
            w = [x for x in near if cur + lo <= x[0] <= cur + hi]
            if w: break
        if not w: cur += STEP; continue
        t, l, f = max(w, key=lambda x: x[2]["score"] / (1 + x[1] / 15))
        stops.append(f); cur = t
    out.append({**a, "pilier": True}); out += [{**f, "pilier": False} for f in stops]
    pts = [a] + stops + [b]; seg = [frame(p, q)[0] for p, q in zip(pts, pts[1:])]; total += sum(seg)
    print(f"\n■ {short(a['name'])} → {short(b['name'])}  {D:.0f} km, plus long saut {max(seg):.0f} km")
    for f in stops: print(f"    · {short(f['name'])[:44]:44} {f['ha']:5d} ha {f['dens']:5.1f} hab/km² {f['town_km']:5.1f} km de {f['town']}")
out.append({**anchors[-1], "pilier": True})
dens = sum(f["dens"] for f in out) / len(out)
print(f"\n{len(out)} forêts dont {len(anchors)} piliers ; {total:.0f} km vol d'oiseau ; densité moyenne {dens:.1f} hab/km²")
json.dump(out, open("fr/diagonale.json", "w"), ensure_ascii=False, indent=0)
