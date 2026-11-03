# Live layer surface changes, judged on a screenshot of a headless KWin. $1 = qs-rs, $2 = qml.
command -v kwin_wayland >/dev/null || { echo "needs a headless KWin (kwin_wayland)"; exit 77; }
python3 -c "import PIL" 2>/dev/null || { echo "needs python3 PIL to read the screenshot"; exit 77; }
out=$(mktemp -d)
QSRS_NEST_W=800 QSRS_NEST_H=600 QSRS_NEST_ROOT=$out/nest sh scripts/nested.sh $out/shot.png 4 "$1" -p "$2" >/dev/null 2>&1
python3 - "$out/shot.png" <<'PY'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
w, h = im.size
red = lambda x, y: im.getpixel((x, y))[0] > 200 and im.getpixel((x, y))[1] < 60
rows = [y for y in range(h) if red(w // 2, y)]
cols = [x for x in range(w) if rows and red(x, rows[0])]
ok = rows and rows[0] == 300 and cols and cols[0] == 100
print(("ok" if ok else "FAILED") + f": red bar rows {rows[:1]}..{rows[-1:]} from x {cols[:1]} (want y 300, x 100)")
sys.exit(0 if ok else 1)
PY
rc=$?; rm -rf "$out"; exit $rc
