# Étapes de la diagonale : d'une forêt à la suivante (fr/diagonale.json).
# Sortie : fr/dg/legs.json au format de fr_leg.sh et fr_gpx.py (une seule « région »).
import json
F = json.load(open("fr/diagonale.json"))
short = lambda n: n.replace("Forêt Domaniale ", "forêt ").replace("Forêt Communale ", "forêt communale ")
legs = [{"region": "Diagonale des forêts isolées", "start": {"lat": a["lat"], "lon": a["lon"]},
         "end": {"lat": b["lat"], "lon": b["lon"]}, "from": short(a["name"]), "to": short(b["name"])}
        for a, b in zip(F, F[1:])]
json.dump(legs, open("fr/dg/legs.json", "w"), ensure_ascii=False, indent=0)
print(len(legs), "étapes")
