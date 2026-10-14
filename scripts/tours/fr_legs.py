# Étapes du tour : 2k = traversée de la forêt k, 2k+1 = liaison vers la forêt k+1.
# Une liaison appartient à la région qu'elle quitte. Sortie : fr/legs.json
import json
F = json.load(open("fr/forests.json")); order = json.load(open("fr/order.json"))
pt = lambda p: {"lat": p["lat"], "lon": p["lon"]}
legs = []
for k, (n, s) in enumerate(order):
    f = F[n]; a, b = (f["a"], f["b"]) if s == 1 else (f["b"], f["a"])
    nx, ns = order[(k + 1) % len(order)]; g = F[nx]
    entry_next = g["a"] if ns == 1 else g["b"]
    legs.append({"kind": "traversée", "region": f["region"], "start": pt(a), "end": pt(b),
                 "from": f"Entrée forêt de {n}", "to": f"Sortie forêt de {n}"})
    legs.append({"kind": "liaison", "region": f["region"], "start": pt(b), "end": pt(entry_next),
                 "from": f"Sortie forêt de {n}", "to": f"Entrée forêt de {nx}"})
json.dump(legs, open("fr/legs.json", "w"), ensure_ascii=False, indent=0)
print(len(legs), "étapes")
