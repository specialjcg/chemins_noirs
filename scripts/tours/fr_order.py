# Ordre du tour : régions d'un seul tenant, forêts traversées a -> b ou b -> a.
# Coût = liaisons à vol d'oiseau (sortie d'une forêt -> entrée de la suivante).
# Sortie : fr/order.json [[nom, sens], ...]  sens 1 = a->b, -1 = b->a
import json, math, random
F = json.load(open("fr/forests.json"))
def d(p, q):
    la1, lo1, la2, lo2 = map(math.radians, (p["lat"], p["lon"], q["lat"], q["lon"]))
    h = math.sin((la2-la1)/2)**2 + math.cos(la1)*math.cos(la2)*math.sin((lo2-lo1)/2)**2
    return 6371*2*math.asin(math.sqrt(h))
ent = lambda n, s: F[n]["a"] if s == 1 else F[n]["b"]
ext = lambda n, s: F[n]["b"] if s == 1 else F[n]["a"]
def cost(seq): return sum(d(ext(*seq[i]), ent(*seq[(i+1) % len(seq)])) for i in range(len(seq)))
regs = {}
for n, f in F.items(): regs.setdefault(f["region"], []).append(n)
random.seed(2)
best = None
for trial in range(40):
    blocks = list(regs.values()); random.shuffle(blocks)
    seq = [(n, random.choice((1, -1))) for b in blocks for n in random.sample(b, len(b))]
    imp = True
    while imp:
        imp = False; c0 = cost(seq)
        # bornes des blocs région
        bounds = []; s = 0
        while s < len(seq):
            e = s
            while e + 1 < len(seq) and F[seq[e+1][0]]["region"] == F[seq[s][0]]["region"]: e += 1
            bounds.append((s, e)); s = e + 1
        cands = []
        for s, e in bounds:  # 2-opt et retournement dans une région
            for i in range(s, e + 1):
                for j in range(i, e + 1):
                    cands.append(seq[:i] + [(n, -k) for n, k in reversed(seq[i:j+1])] + seq[j+1:])
        for x in range(len(bounds)):  # 2-opt sur l'ordre des régions
            for y in range(x + 1, len(bounds)):
                i, j = bounds[x][0], bounds[y][1]
                cands.append(seq[:i] + [(n, -k) for n, k in reversed(seq[i:j+1])] + seq[j+1:])
        for c in cands:
            if cost(c) < c0 - 1e-6: seq, imp = c, True; break
    if best is None or cost(seq) < cost(best): best = seq
trav = sum(F[n]["axis_km"] for n, _ in best)
print(f"liaisons {cost(best):.0f} km + traversées {trav:.0f} km (vol d'oiseau)")
cur = None
for k, (n, s) in enumerate(best):
    if F[n]["region"] != cur: cur = F[n]["region"]; print(f"== {cur}")
    nx = best[(k+1) % len(best)]
    print(f"  {k:2} {n[:30]:30} {F[n]['axis_km']:5.1f} km  -> liaison {d(ext(n, s), ent(*nx)):5.1f}")
json.dump(best, open("fr/order.json", "w"), ensure_ascii=False)
