# Judges a springbench run: see run.sh.
import json, math, re, statistics, sys
out = sys.argv[1]
log = open(out + "/run.log", errors="replace").read()
m = re.search(r"FRAMES (\[.*\])", log)
if not m: print("no frames"); print(log[-800:]); sys.exit(1)
fr = json.loads(m[1])
P = 1000 / 60
w = 2 * math.pi / 300
t0 = fr[0][0]
pts = [(t - t0, v) for t, v, _ in fr]
segs, cur, last = [], [], None
for (t, v), f in zip(pts, fr):
    if f[2] != last and cur: segs.append(cur); cur = []
    last = f[2]
    cur.append((t, v))
if cur: segs.append(cur)
res = []
for s in segs:
    to = 400 if s[-1][1] > 200 else 0
    frm = 400 - to
    mov = [(t, v) for i, (t, v) in enumerate(s) if i > 0 and 12 <= t - s[i - 1][0] <= 22 and t - s[0][0] > 100 and abs(v - to) > 0.5]
    if len(mov) < 4: continue
    def ideal(tt):
        if tt <= 0: return frm
        return to + (frm - to) * (1 + w * tt) * math.exp(-w * tt)
    best = None
    base = s[0][0]
    for k in range(-4000, 4000):
        phi = base + k * 0.01
        e = [v - ideal(t - phi) for t, v in mov]
        r = math.sqrt(sum(x * x for x in e) / len(e))
        if best is None or r < best[0]: best = (r, max(abs(x) for x in e))
    res.append(best)
try:
    ticks = [float(dict(kv.split("=", 1) for kv in l.split())["tick"]) for l in open(out + "/frames.log")]
except OSError:
    ticks = []
rms = statistics.mean(r for r, _ in res) if res else float("nan")
mx = max((m for _, m in res), default=float("nan"))
print(f"segments {len(res)} jitter rms {rms:.3f} px max {mx:.3f} px; tick mean {statistics.mean(ticks) if ticks else float('nan'):.3f} ms p95 {sorted(ticks)[int(len(ticks)*0.95)] if ticks else float('nan'):.3f} ms over {len(ticks)} frames")
