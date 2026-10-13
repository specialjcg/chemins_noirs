import json, sys, urllib.parse, urllib.request
NAMES = json.load(open(sys.argv[2]))
def area(r):
    return abs(sum(r[i][0]*r[i+1][1]-r[i+1][0]*r[i][1] for i in range(len(r)-1)))/2
def inside(p, r):
    x, y, c = p[0], p[1], False
    for i in range(len(r)-1):
        (x1,y1),(x2,y2) = r[i][:2], r[i+1][:2]
        if (y1>y)!=(y2>y) and x < x1+(y-y1)*(x2-x1)/(y2-y1): c = not c
    return c
def rings(g):
    return [g["coordinates"][0]] if g["type"]=="Polygon" else [p[0] for p in g["coordinates"]]
out = {}
for name, pat in NAMES.items():
    q = urllib.parse.urlencode({"service":"WFS","version":"2.0.0","request":"GetFeature","typeNames":"BDTOPO_V3:foret_publique",
        "outputFormat":"application/json","srsName":"EPSG:4326","cql_filter":f"toponyme ILIKE '{pat}'"})
    fc = json.load(urllib.request.urlopen("https://data.geopf.fr/wfs/ows?"+q, timeout=60))
    feats = []
    for f in fc["features"]:
        rs = rings(f["geometry"])
        feats.append((sum(area(r) for r in rs), f["properties"].get("toponyme"), rs))
    if not feats: print(f"{name}: RIEN"); continue
    feats.sort(key=lambda t: -t[0])
    a, topo, rs = feats[0]
    big = max(rs, key=area)
    cx = sum(p[0] for p in big)/len(big); cy = sum(p[1] for p in big)/len(big)
    if not inside((cx,cy), big):  # repli : point intérieur sur la ligne horizontale du centre
        xs = sorted(x1+(cy-y1)*(x2-x1)/(y2-y1) for (x1,y1,*_),(x2,y2,*_) in zip(big,big[1:]) if (y1>cy)!=(y2>cy))
        cx = (xs[0]+xs[1])/2
    allp = [p for r in rs for p in r]
    out[name] = {"toponyme": topo, "lat": round(cy,5), "lon": round(cx,5), "km2": round(a*111*78,1),
                 "bbox": [min(p[1] for p in allp), min(p[0] for p in allp), max(p[1] for p in allp), max(p[0] for p in allp)],
                 "others": [t[1] for t in feats[1:4]]}
    print(f"{name:26} {topo:45.45} {cy:.4f} {cx:.4f} ~{a*111*78:.0f} km² | autres: {[t[1] for t in feats[1:3]]}")
json.dump(out, open(sys.argv[1], "w"), ensure_ascii=False, indent=1)
