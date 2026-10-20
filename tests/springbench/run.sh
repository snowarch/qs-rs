# SpringValue against inir's IrisSpring in one bench: a 60 Hz headless KWin, N springs going
# 0 -> 400 -> 0 every 900 ms (300 ms, no bounce), LOAD ms of random JS after each frame.
# Prints the error of spring 0 against the closed form at each frame's swap time (px) and the
# animation step's cost from the frame meter (ms).
#   tests/springbench/run.sh QS IMPL(native|js) [N] [LOAD] [IrisSpring.qml]
qs=$(realpath "$1"); impl=$2; n=${3:-1}; load=${4:-0}
src=${5:-$HOME/inir-eco/inir/modules/iris/components/IrisSpring.qml}
here=$(dirname "$(realpath "$0")")
out=$(mktemp -d)
cp "$here/shell.qml" "$out/shell.qml"
# IrisSpring with fixed parameters and its own bezier in place of the IrisStyle singleton.
python3 - "$src" "$HOME/inir-eco/inir/modules/iris/style/IrisStyle.qml" "$out/JsSpring.qml" <<'PY'
import re, sys
s = open(sys.argv[1]).read()
s = s.replace("import qs.modules.iris.style\n", "")
s = s.replace("IrisStyle.springFor(wanted, root.surface)", "root.springParams")
s = re.sub(r"readonly property var motionSignature: \[.*?\]\]?\n", "", s, flags=re.S)
s = s.replace("onMotionSignatureChanged: root.forget()\n", "")
s = s.replace("IrisStyle.cubicBezier(", "root.bez(")
assert "IrisStyle" not in s
style = open(sys.argv[2]).read()
i = style.index("    function cubicBezier(curve: var, x: real): real {")
j = style.index("\n    }\n", i) + 7
s = s.replace("    property bool running: false", "    property var springParams: ({ response: 300, bounce: 0 })\n"
              + style[i:j].replace("cubicBezier", "bez") + "    property bool running: false", 1)
open(sys.argv[3], "w").write(s)
PY
QSRS_EXTRA_ENV="IMPL=$impl N=$n LOAD=$load" QSRS_NEST_W=800 QSRS_NEST_H=600 QSRS_NEST_ROOT=$out/nest \
  sh scripts/nested.sh $out/shot.png 11 sh -c '
  "$1" -p "$2" >"$3/run.log" 2>&1 &
  for i in $(seq 40); do "$1" frames start "$3/frames.log" -p "$2" >/dev/null 2>&1 && break; sleep 0.1; done
  wait' sh "$qs" "$out/shell.qml" "$out" >/dev/null 2>&1
python3 "$here/judge.py" "$out"; rc=$?
rm -rf "$out"; exit $rc
