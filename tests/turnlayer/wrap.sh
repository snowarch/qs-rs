# A bar turned from the bottom edge to a side, judged on a screenshot of niri nested in a headless KWin (KWin itself
# drew it right; niri drew nothing). $1 = qs-rs, $2 = qml.
command -v kwin_wayland >/dev/null || { echo "needs a headless KWin (kwin_wayland)"; exit 77; }
command -v niri >/dev/null && command -v grim >/dev/null || { echo "needs niri and grim"; exit 77; }
python3 -c "import PIL" 2>/dev/null || { echo "needs python3 PIL to read the screenshot"; exit 77; }
out=$(mktemp -d)
QSRS_NIRI_WAIT=1 QSRS_NEST_W=800 QSRS_NEST_H=600 QSRS_NEST_ROOT=$out/nest sh scripts/nested-niri.sh $out/shot.png 4 "$1" -p "$2" >/dev/null 2>&1
python3 - "$out/shot.png" <<'PY'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
w, h = im.size
red = lambda x, y: im.getpixel((x, y))[0] > 200 and im.getpixel((x, y))[1] < 60
rows = [y for y in range(h) if red(30, y)]
cols = [x for x in range(w) if red(x, h // 2)]
ok = rows[:1] == [0] and rows[-1:] == [h - 1] and cols[:1] == [0] and cols[-1:] == [59]
print(("ok" if ok else "FAILED") + f": red at x 30 on rows {rows[:1]}..{rows[-1:]}, at y {h // 2} on columns {cols[:1]}..{cols[-1:]} (want rows 0..{h - 1}, columns 0..59)")
sys.exit(0 if ok else 1)
PY
rc=$?; rm -rf "$out"; exit $rc
