#!/bin/sh
# The software renderer offscreen has no updatesEnabled gate: run the test on GL in a headless
# KWin instead. Args: <qs-rs> <test qml>.
command -v kwin_wayland >/dev/null || { echo "needs a headless KWin (kwin_wayland)"; exit 77; }
root=$(mktemp -d "${TMPDIR:-/tmp}/qsn.XXXXXX")
out=$(QSRS_NEST_ROOT=$root scripts/nested.sh "$root/shot.png" 6 sh -c "$1 -p $(realpath "$2"); echo EXIT=\$?" 2>&1)
rm -rf "$root"
echo "$out" | grep -E "FAILED|EXIT"
echo "$out" | grep -qx "EXIT=0"
