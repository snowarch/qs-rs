# Frame pacing and the frame meter on a headless KWin (60 Hz). $1 = qs-rs, $2 = qml.
# The meter is turned on over IPC (`qs frames start FILE`), as a script would on a live shell.
out=$(mktemp -d)
qs=$(realpath "$1"); qml=$(realpath "$2")
QSRS_NEST_W=800 QSRS_NEST_H=600 QSRS_NEST_ROOT=$out/nest sh scripts/nested.sh $out/shot.png 8 sh -c '
  "$1" -p "$2" >"$3/run.log" 2>&1 &
  for i in $(seq 40); do "$1" frames start "$3/frames.log" -p "$2" >/dev/null 2>&1 && break; sleep 0.1; done
  sleep 6.5
  "$1" frames stop -p "$2" >"$3/stats.json" 2>&1
  wait' sh "$qs" "$qml" "$out" >/dev/null 2>&1
python3 - "$out" <<'PY'
import json, re, statistics, sys
out = sys.argv[1]
log = open(f"{out}/run.log").read()
phases = {m[0]: (int(m[1]), int(m[2])) for m in re.findall(r"phase (\w+) steps (\d+) frames (\d+)", log)}
try: lines = [dict(kv.split("=", 1) for kv in l.split()) for l in open(f"{out}/frames.log")]
except OSError: lines = []
light = [l for l in lines if float(l["tick"]) < 5 and l["cont"] == "1"]
load = [l for l in lines if float(l["tick"]) >= 20]
try: stats = json.loads(open(f"{out}/stats.json").read())
except (OSError, ValueError): stats = {"windows": []}
win = stats["windows"][0] if stats["windows"] else {}
checks = []
def check(name, ok, got):
    checks.append(ok)
    print(("ok   " if ok else "FAIL ") + f"{name}: {got}")
s, f = phases.get("light", (0, 1))
check("light: one step per frame", f > 0 and s / f <= 1.15, f"{s} steps, {f} frames in 2 s")
check("light: 60 frames a second", f >= 110, f"{f / 2:.1f} fps")
md = statistics.median(float(l["d"]) for l in light) if light else 0
check("meter light: frames 16.7 ms apart", abs(md - 16.67) < 1.5, f"median d {md:.2f} ms over {len(light)} frames")
miss = sum(int(l["miss"]) for l in light)
check("meter light: nothing missed", miss <= max(2, len(light) // 30), f"{miss} missed")
s, f = phases.get("load", (0, 1))
check("load: still one step per frame", f > 0 and s / f <= 1.15, f"{s} steps, {f} frames in 2 s")
md = statistics.median(float(l["d"]) for l in load) if load else 0
# A headless KWin has no vblank: the frame goes out when the step is done (25 ms), not at 33.
check("meter load: frames over 1.2 refreshes apart", 24 <= md <= 36, f"median d {md:.2f} ms over {len(load)} frames")
mm = statistics.mean(int(l["miss"]) for l in load) if load else 0
check("meter load: one refresh missed per frame", 0.8 <= mm <= 1.3, f"mean miss {mm:.2f}")
mt = statistics.median(float(l["tick"]) for l in load) if load else 0
check("meter load: the step's 25 ms show", 24 <= mt <= 32, f"median tick {mt:.2f} ms")
check("stats: window named, rate known", win.get("win") == "framepace.qml" and abs(win.get("hz", 0) - 60) < 1, f"{win.get('win')} at {win.get('hz')} Hz")
sys.exit(0 if all(checks) else 1)
PY
rc=$?; [ -n "${QSRS_KEEP:-}" ] && echo "kept $out" || rm -rf "$out"; exit $rc
