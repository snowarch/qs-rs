# A window held by two BackgroundEffects, in a nested niri (the compositor that enforces one
# effect object per wl_surface). $1 = qs-rs, $2 = qml. The client's protocol log says which
# region each blur request sent: the second holder's while it holds, the window's own after.
command -v niri >/dev/null || { echo "needs a nested niri (niri)"; exit 77; }
out=$(mktemp -d)
QSRS_NEST_ROOT=$out/nest sh scripts/niri-nested.sh 3 env WAYLAND_DEBUG=client "$(realpath "$1")" -p "$(realpath "$2")"
python3 - "$out/nest/app.log" <<'PY'
import re, sys
log = open(sys.argv[1], errors="replace").read().splitlines()
if any("already has a background effect" in l for l in log):
    print("FAIL: " + next(l for l in log if "already has a background effect" in l)); sys.exit(1)
rects, sent, phase = {}, [], "first"
for l in log:
    if "second holder attached" in l: phase = "both"
    if "second holder let go" in l: phase = "after"
    m = re.search(r"-> wl_region#(\d+)\.add\((-?\d+), (-?\d+), (\d+), (\d+)\)", l)
    if m: rects[m[1]] = (int(m[4]), int(m[5]))
    m = re.search(r"-> ext_background_effect_surface_v1#\d+\.set_blur_region\((?:wl_region#(\d+)|nil)\)", l)
    if m: sent.append((phase, rects.get(m[1]) if m[1] else None))
gets = sum("get_background_effect" in l and "->" in l for l in log)
last = {p: [r for q, r in sent if q == p][-1:] for p in ("first", "both", "after")}
ok = gets == 1 and last["first"] == [(100, 50)] and last["both"] == [(40, 40)] and last["after"] == [(100, 50)]
print(("ok" if ok else "FAIL") + f": {gets} effect object(s); last region sent alone {last['first']}, with two holders {last['both']}, after {last['after']}")
sys.exit(0 if ok else 1)
PY
rc=$?; rm -rf "$out"; exit $rc
