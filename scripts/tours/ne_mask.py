# usage: ne_mask.py S leg.json out.geojson — masque forêt IGN des tuiles 0,1° traversées par le tracé
import json, sys, os, math, subprocess
S, leg, out = sys.argv[1:4]
path = json.load(open(leg))["path"]
tiles = {(math.floor(p["lon"]*10), math.floor(p["lat"]*10)) for p in path}
feats = []
for tx, ty in sorted(tiles):
    f = f"{S}/ne/tiles/{tx}_{ty}.geojson"
    if not os.path.exists(f) or os.path.getsize(f) < 100:
        subprocess.run(["curl", "-s", "-G", "https://data.geopf.fr/wfs/ows", "--data-urlencode", "service=WFS",
            "--data-urlencode", "version=2.0.0", "--data-urlencode", "request=GetFeature",
            "--data-urlencode", "typeNames=IGNF_MASQUE-FORET.2021-2023:masque_foret",
            "--data-urlencode", "outputFormat=application/json", "--data-urlencode", "srsName=CRS:84",
            "--data-urlencode", "count=10000", "--data-urlencode",
            f"CQL_FILTER=BBOX(geom,{tx/10},{ty/10},{(tx+1)/10},{(ty+1)/10},'urn:ogc:def:crs:OGC:1.3:CRS84')",
            "-o", f], check=True)
    fs = json.load(open(f))["features"]
    if len(fs) >= 10000: print("ATTENTION tuile pleine", tx, ty)
    feats += fs
json.dump({"type": "FeatureCollection", "features": feats}, open(out, "w"))
print(f"{len(tiles)} tuiles, {len(feats)} entités")
