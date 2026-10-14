# Isolement des forêts publiques : population à moins de 10 km du centre,
# distance à la première ville de 5 000 habitants. Sortie : fr/remote.json
import json, math
from collections import defaultdict
I = [f for f in json.load(open("fr/index.json")) if f["ha"] >= int(__import__("os").environ.get("MIN_HA", 1500))]
C = [(c["centre"]["coordinates"][1], c["centre"]["coordinates"][0], c["population"], c["nom"])
     for c in json.load(open("fr/communes.json")) if c.get("centre") and c.get("population")]
grid = defaultdict(list)
for c in C: grid[int(c[0] * 10), int(c[1] * 10)].append(c)
def km(a, b, c, d):
    return math.hypot((d - b) * 111.32 * math.cos(math.radians((a + c) / 2)), (c - a) * 110.54)
def around(lat, lon, r):
    n = int(r / 8) + 1
    for i in range(int(lat * 10) - n, int(lat * 10) + n + 1):
        for j in range(int(lon * 10) - n, int(lon * 10) + n + 1): yield from grid[i, j]
towns = [c for c in C if c[2] >= 5000]
for f in I:
    f["pop10"] = sum(c[2] for c in around(f["lat"], f["lon"], 10) if km(f["lat"], f["lon"], c[0], c[1]) <= 10)
    t = min(towns, key=lambda c: km(f["lat"], f["lon"], c[0], c[1]))
    f["town_km"], f["town"] = round(km(f["lat"], f["lon"], t[0], t[1]), 1), t[3]
    # densité autour (hab/km² sur 10 km) ; isolement = loin des villes, peu habité, grand
    f["dens"] = round(f["pop10"] / (math.pi * 100), 1)
    f["score"] = round(f["town_km"] / (1 + f["dens"]) * math.log10(f["ha"]), 2)
I.sort(key=lambda f: -f["score"])
json.dump(I, open(__import__("os").environ.get("OUT", "fr/remote.json"), "w"), ensure_ascii=False)
print(f"{len(I)} forêts ≥ 1500 ha ; médiane densité {sorted(f['dens'] for f in I)[len(I)//2]} hab/km²")
for f in I[:35]:
    print(f"{f['score']:6.1f}  {f['name'][:52]:52} {f['ha']:6d} ha  {f['dens']:5.1f} hab/km²  {f['town_km']:5.1f} km de {f['town']}")
