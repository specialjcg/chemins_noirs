# Forêts domaniales de métropole (hors Corse) d'axe >= MIN km, avec leur région.
# Sortie : fr/forests.json {nom: {toponyme, a, b, lat, lon, axis_km, inside, region}}
import json, sys, urllib.request, time
MIN = float(sys.argv[1]) if len(sys.argv) > 1 else 15
axes = json.load(open("fr/axes.json"))
regions = {r["code"]: r["nom"] for r in json.load(urllib.request.urlopen("https://geo.api.gouv.fr/regions"))}
def region(lat, lon):
    u = f"https://geo.api.gouv.fr/communes?lat={lat}&lon={lon}&fields=codeRegion&format=json"
    for _ in range(3):
        try:
            r = json.load(urllib.request.urlopen(u, timeout=20))
            if r: return regions.get(r[0]["codeRegion"])
        except Exception: time.sleep(2)
    return None
out = {}
for f in axes:
    if f["axis_km"] < MIN or "a" not in f: continue
    (la, oa), (lb, ob) = f["a"], f["b"]
    lat, lon = (la + lb) / 2, (oa + ob) / 2
    if lon > 8.4 and lat < 43.1: continue  # Corse
    reg = region(lat, lon) or region(la, oa) or region(lb, ob)
    name = f["name"].replace("Forêt Domaniale ", "").replace("Indivise ", "").strip()
    for p in ("de la ", "de l'", "des ", "du ", "de ", "d'"):
        if name.startswith(p): name = name[len(p):]; break
    out[name] = {"toponyme": f["name"], "a": {"lat": la, "lon": oa}, "b": {"lat": lb, "lon": ob},
                 "lat": lat, "lon": lon, "axis_km": f["axis_km"], "inside": f["inside"], "region": reg}
json.dump(out, open("fr/forests.json", "w"), ensure_ascii=False, indent=0)
from collections import Counter
print(len(out), "forêts"); print(Counter(v["region"] for v in out.values()).most_common())
