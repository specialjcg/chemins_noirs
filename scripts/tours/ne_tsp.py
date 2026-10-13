import json, math, sys, itertools, random
F = json.load(open(sys.argv[1])); names = list(F)
def d(a, b):
    la1, lo1, la2, lo2 = map(math.radians, (F[a]["lat"], F[a]["lon"], F[b]["lat"], F[b]["lon"]))
    h = math.sin((la2-la1)/2)**2 + math.cos(la1)*math.cos(la2)*math.sin((lo2-lo1)/2)**2
    return 6371*2*math.asin(math.sqrt(h))
def L(t): return sum(d(t[i], t[(i+1)%len(t)]) for i in range(len(t)))
best = None
random.seed(1)
for _ in range(300):
    t = names[:]; random.shuffle(t); imp = True
    while imp:
        imp = False
        for i, j in itertools.combinations(range(1, len(t)), 2):
            n = t[:i] + t[i:j+1][::-1] + t[j+1:]
            if L(n) < L(t) - 1e-9: t, imp = n, True
    if best is None or L(t) < L(best): best = t
i = best.index("Montagne de Reims"); best = best[i:] + best[:i]
for k, n in enumerate(best): print(f"{k:2} {n:26} -> {d(n, best[(k+1)%len(best)]):6.1f} km")
print(f"total vol d'oiseau {L(best):.0f} km")
json.dump(best, open(sys.argv[2], "w"), ensure_ascii=False)
