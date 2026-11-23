# A dock the compositor narrows, judged on a screenshot of niri nested in a headless KWin. $1 = qs-rs, $2 = qml.
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
cols = [x for x in range(w) if red(x, h - 30)]
ok = cols[:1] == [250] and cols[-1:] == [449]
print(("ok" if ok else "FAILED") + f": red at y {h - 30} on columns {cols[:1]}..{cols[-1:]} (want 250..449)")
sys.exit(0 if ok else 1)
PY
rc=$?; rm -rf "$out"; exit $rc
