# usage: fr_gpx.py DIR OUTDIR — un GPX par région (fr-NN-slug.gpx) à partir des leg_XX.json (mix)
# Points nommés : wpt[0] = départ, wpt[i+1] = fin de la trace i (format attendu par reg_split30).
import json, sys, re, unicodedata
from xml.sax.saxutils import escape
D, out = sys.argv[1:3]
legs = json.load(open(f"{D}/legs.json"))
regions = []
for l in legs:
    if l["region"] not in regions: regions.append(l["region"])
def slug(s): return re.sub(r"[^a-z]+", "-", unicodedata.normalize("NFD", s).encode("ascii", "ignore").decode().lower()).strip("-")
for r_i, reg in enumerate(regions):
    idx = [i for i, l in enumerate(legs) if l["region"] == reg]
    g = ['<?xml version="1.0" encoding="UTF-8"?>', '<gpx version="1.1" creator="chemins-noirs" xmlns="http://www.topografix.com/GPX/1/1">',
         f'<metadata><name>{escape("Tour de France des forêts — " + reg)}</name></metadata>']
    s0 = legs[idx[0]]["start"]; g.append(f'<wpt lat="{s0["lat"]}" lon="{s0["lon"]}"><name>{escape(legs[idx[0]]["from"])}</name></wpt>')
    for i in idx:
        e = legs[i]["end"]; g.append(f'<wpt lat="{e["lat"]}" lon="{e["lon"]}"><name>{escape(legs[i]["to"])}</name></wpt>')
    km = 0
    for i in idx:
        j = json.load(open(f"{D}/leg_{i:02d}.json")); ele = j["elevation_profile"]["elevations"]; km += j["distance_km"]
        g.append(f'<trk><name>{escape(legs[i]["from"])} - {escape(legs[i]["to"])}</name><trkseg>')
        g += [f'<trkpt lat="{p["lat"]:.6f}" lon="{p["lon"]:.6f}"><ele>{e:.1f}</ele></trkpt>' for p, e in zip(j["path"], ele)]
        g.append('</trkseg></trk>')
    g.append('</gpx>')
    path = f"{out}/fr-{r_i+1:02d}-{slug(reg)}.gpx"
    open(path, "w").write("\n".join(g))
    print(f"{path}\t{reg}\t{km:.0f}")
