import sys, json, time, urllib.request
B = "http://localhost:8090/api"
def call(url, body=None, method=None):
    req = urllib.request.Request(url, json.dumps(body).encode() if body is not None else None,
                                 {"Content-Type": "application/json"}, method=method)
    return json.load(urllib.request.urlopen(req, timeout=3500))
for i in range(int(sys.argv[1]), int(sys.argv[2]) + 1):
    t0 = time.time()
    r = call(f"{B}/routes/{i}")
    route = r["route_data"]
    s = call(f"{B}/surfaces", {"path": route["path"]})
    route["point_surfaces"] = s["point_surfaces"]
    if s.get("point_ign") is not None: route["point_ign"] = s["point_ign"]
    meta = {"name": r["name"], "description": r["description"], "tags": r["tags"]}
    call(f"{B}/routes/{i}", [meta, route], "PUT")
    n = len(route["path"]); unp = s["point_surfaces"].count(False); ign = (s.get("point_ign") or []).count(True)
    print(f"{i} {r['name']} {n} pts  terre {100*unp/n:.0f}%  ign {100*ign/n:.0f}%  {time.time()-t0:.0f}s", flush=True)
