import re, sys, math, os
src, outdir = sys.argv[1], sys.argv[2]
MAX, SNAP = 30.0, 2.0
txt = open(src, encoding="utf-8").read()
wpts = [(float(a), float(b), n) for a, b, n in re.findall(r'<wpt lat="([^"]+)" lon="([^"]+)"><name>([^<]*)</name>', txt)]
trks = re.findall(r'<trk>(.*?)</trk>', txt, re.S)
pt_re = re.compile(r'<trkpt lat="([^"]+)" lon="([^"]+)"><ele>([^<]+)</ele>')
def hav(a, b):
    R = 6371.0
    la1, lo1, la2, lo2 = map(math.radians, (a[0], a[1], b[0], b[1]))
    h = math.sin((la2-la1)/2)**2 + math.cos(la1)*math.cos(la2)*math.sin((lo2-lo1)/2)**2
    return 2*R*math.asin(math.sqrt(h))
pts, bounds = [], {}  # bounds: index -> forest name (leg boundaries)
for i, t in enumerate(trks):
    p = [(float(a), float(b), float(e)) for a, b, e in pt_re.findall(t)]
    if pts and hav(pts[-1], p[0]) < 0.05: p = p[1:]
    pts += p
    bounds[len(pts)-1] = wpts[(i+1) % len(wpts)][2]
bounds_start = {0: wpts[0][2]}
cum = [0.0]
for a, b in zip(pts, pts[1:]): cum.append(cum[-1] + hav(a, b))
cuts, s = [0], 0
while cum[-1] - cum[s] > MAX:
    lim = cum[s] + MAX
    e = max(i for i in range(s, len(pts)) if cum[i] <= lim)
    snap = [i for i in bounds if s < i <= e and cum[i] >= lim - SNAP]
    e = max(snap) if snap else e
    cuts.append(e); s = e
cuts.append(len(pts)-1)
# merge tiny last piece (<5 km) into previous if result stays <= MAX+5
os.makedirs(outdir, exist_ok=True)
names = {**bounds_start, **bounds}
n = len(cuts)-1
for k in range(n):
    a, b = cuts[k], cuts[k+1]
    km = cum[b]-cum[a]
    na, nb = names.get(a, f"Coupe km {cum[a]:.0f}"), names.get(b, f"Coupe km {cum[b]:.0f}")
    title = f"Tour forêts NE {k+1:02d}/{n} — {km:.1f} km"
    seg = "\n".join(f'<trkpt lat="{p[0]:.6f}" lon="{p[1]:.6f}"><ele>{p[2]:.1f}</ele></trkpt>' for p in pts[a:b+1])
    w = lambda i, nm: f'<wpt lat="{pts[i][0]:.6f}" lon="{pts[i][1]:.6f}"><ele>{pts[i][2]:.1f}</ele><name>{nm}</name></wpt>'
    out = (f'<?xml version="1.0" encoding="UTF-8"?>\n<gpx version="1.1" creator="chemins-noirs" xmlns="http://www.topografix.com/GPX/1/1">\n'
           f'<metadata><name>{title}</name></metadata>\n{w(a, "Départ — "+na)}\n{w(b, "Arrivée — "+nb)}\n'
           f'<trk><name>{title}</name><trkseg>\n{seg}\n</trkseg></trk>\n</gpx>\n')
    open(f"{outdir}/tour-forets-ne-{k+1:02d}.gpx", "w", encoding="utf-8").write(out)
    print(f"{k+1:02d} {km:5.1f} km  {na[:28]:28} -> {nb[:28]}")
print("total", round(cum[-1], 1), "km,", n, "fichiers")
