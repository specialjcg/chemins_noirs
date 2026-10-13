import re, sys, json, math, base64, glob, urllib.request
D = sys.argv[1]
def hav(a, b):
    la1, lo1, la2, lo2 = map(math.radians, (a[0], a[1], b[0], b[1]))
    h = math.sin((la2-la1)/2)**2 + math.cos(la1)*math.cos(la2)*math.sin((lo2-lo1)/2)**2
    return 2*6371*math.asin(math.sqrt(h))
files = sorted(glob.glob(f"{D}/tour-forets-ne-*.gpx"))
for f in files:
    raw = open(f, "rb").read(); t = raw.decode()
    pts = [(float(a), float(b), float(e)) for a, b, e in re.findall(r'<trkpt lat="([^"]+)" lon="([^"]+)"><ele>([^<]+)</ele>', t)]
    title = re.search(r'<metadata><name>([^<]+)', t).group(1)
    dep, arr = re.findall(r'<wpt[^>]*><ele>[^<]*</ele><name>([^<]+)', t)
    ele = [p[2] for p in pts]
    up = sum(max(0, b-a) for a, b in zip(ele, ele[1:])); dn = sum(max(0, a-b) for a, b in zip(ele, ele[1:]))
    km = sum(hav(a, b) for a, b in zip(pts, pts[1:]))
    route = {"path": [{"lat": p[0], "lon": p[1]} for p in pts], "distance_km": km,
             "gpx_base64": base64.b64encode(raw).decode(),
             "elevation_profile": {"elevations": ele, "min_elevation": min(ele), "max_elevation": max(ele),
                                   "total_ascent": round(up, 1), "total_descent": round(dn, 1)}}
    name = f.rsplit("/", 1)[1][:-4]
    body = [{"name": name, "description": f"{title} · {dep} → {arr}", "tags": ["tour-forets-nord-est"]}, route]
    req = urllib.request.Request("http://localhost:8090/api/routes", json.dumps(body).encode(), {"Content-Type": "application/json"})
    try: r = json.load(urllib.request.urlopen(req, timeout=60))
    except urllib.error.HTTPError as e: print(e.read()[:400]); sys.exit(1)
    print(r["id"], name, round(km, 1))
