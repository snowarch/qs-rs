# Per-screen panels on two outputs, judged on a screenshot of a headless KWin. $1 = qs-rs, $2 = qml.
command -v kwin_wayland >/dev/null || { echo "needs a headless KWin (kwin_wayland)"; exit 77; }
out=$(mktemp -d)
QSRS_NEST_W=800 QSRS_NEST_H=600 QSRS_NEST_OUTPUTS=2 QSRS_NEST_ROOT=$out/nest sh scripts/nested.sh $out/shot.png 4 "$1" -p "$2" >/dev/null 2>&1
python3 - "$out/shot.png" <<'PY'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
w, h = im.size
def color(x, y):
    r, g, b = im.getpixel((x, y))
    return "red" if r > 200 and g < 60 and b < 60 else "lime" if g > 200 and r < 60 and b < 60 else "blue" if b > 200 and r < 60 and g < 60 else "-"
got = [color(w // 4, 30), color(3 * w // 4, 30), color(w // 4, h - 30), color(3 * w // 4, h - 30)]
want = ["red", "lime", "-", "blue"]
print(("ok" if got == want and w == 1600 else "FAILED") + f": {w}x{h} top {got[:2]} bottom {got[2:]} (want {want})")
sys.exit(0 if got == want and w == 1600 else 1)
PY
rc=$?; rm -rf "$out"; exit $rc
