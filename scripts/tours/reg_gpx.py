# usage: reg_gpx.py DIR title out.gpx — assemble DIR/leg_XX.json (choix mix) + waypoints forêts
import json, sys
from xml.sax.saxutils import escape
D, title, out = sys.argv[1:4]
F = json.load(open(f"{D}/forests.json")); order = json.load(open(f"{D}/order.json"))
g = ['<?xml version="1.0" encoding="UTF-8"?>', '<gpx version="1.1" creator="chemins-noirs" xmlns="http://www.topografix.com/GPX/1/1">',
     f'<metadata><name>{escape(title)}</name></metadata>']
for n in order:
    f = F[n]; g.append(f'<wpt lat="{f["lat"]}" lon="{f["lon"]}"><name>{escape(f["toponyme"])}</name></wpt>')
for i, n in enumerate(order):
    j = json.load(open(f"{D}/leg_{i:02d}.json")); ele = j["elevation_profile"]["elevations"]
    g.append(f'<trk><name>{i+1:02d} {escape(n)} - {escape(order[(i+1) % len(order)])}</name><trkseg>')
    g += [f'<trkpt lat="{p["lat"]:.6f}" lon="{p["lon"]:.6f}"><ele>{e:.1f}</ele></trkpt>' for p, e in zip(j["path"], ele)]
    g.append('</trkseg></trk>')
g.append('</gpx>')
open(out, "w").write("\n".join(g))
