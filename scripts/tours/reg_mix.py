# usage: reg_mix.py DIR N — leg_XX.json = w30 sauf si w30 > 1.3 × w5 (ou w30 absent)
import json, sys, shutil
D, N = sys.argv[1], int(sys.argv[2])
def load(p):
    try:
        j = json.load(open(p)); return j if j.get("path") else None
    except Exception: return None
tot = 0
for i in range(N):
    a, b = load(f"{D}/leg_{i:02d}_w30.json"), load(f"{D}/leg_{i:02d}_w5.json")
    pick = 30 if a and (not b or a["distance_km"] <= 1.3 * b["distance_km"]) else 5
    if pick == 5 and not b: sys.exit(f"étape {i} : aucun calcul valide")
    shutil.copy(f"{D}/leg_{i:02d}_w{pick}.json", f"{D}/leg_{i:02d}.json")
    km = (a if pick == 30 else b)["distance_km"]; tot += km
    print(f"leg {i:02d} w{pick}  {km:6.1f} km  (w30 {a['distance_km'] if a else '-'}  w5 {b['distance_km'] if b else '-'})")
print(f"total {tot:.0f} km")
