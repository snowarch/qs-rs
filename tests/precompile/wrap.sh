#!/bin/sh
# Precompile workers on a shell of 130 parts: its first start compiles every file ahead (nothing compiled yet); an
# "update" that changes 125 parts compiles those again; a start with nothing changed, or only five files, starts no
# worker. Each start must still run the shell. Args: <qs-rs> <placeholder qml>.
set -u
bin=$1
dir=$XDG_STATE_HOME/precompile-shell
mkdir -p "$dir/parts"
parts() { i=0; while [ $i -lt 130 ]; do
  v=v1; [ $i -lt "$1" ] && v=$2
  printf 'import QtQuick\nItem { property int n: %d; property string s: "%s" }\n' $i $v > "$dir/parts/Part$i.qml"
  i=$((i + 1)); done; }
parts 0 v1
{ printf 'import QtQuick\nimport Quickshell\nimport "parts"\nShellRoot {\n'
  i=0; while [ $i -lt 130 ]; do printf '    Part%d {}\n' $i; i=$((i + 1)); done
  printf '    Component.onCompleted: Qt.exit(0)\n}\n'; } > "$dir/shell.qml"
start() { NO_COLOR=1 timeout 15 "$bin" -v -p "$dir/shell.qml" 2>&1; }
ahead() { echo "$1" | grep -o "Compiled [0-9]* QML files ahead in [0-9]* workers" ; }
fail() { echo "FAIL: $1"; echo "$2" | grep -E "QML cache|ahead|ERROR|WARN" | head -12; exit 1; }

out=$(start) || fail "first start did not run the shell" "$out"
echo "$out" | grep -q "Compiled 131 QML files ahead in [0-9]* workers, [0-9]* ms$" || fail "first start did not compile every file ahead" "$out"
out=$(start) || fail "a warm start did not run the shell" "$out"
[ -z "$(ahead "$out")" ] || fail "a warm start compiled ahead" "$out"
parts 125 v2
out=$(start) || fail "the start after an update did not run the shell" "$out"
ahead "$out" | grep -q "Compiled 125 QML files ahead" || fail "the start after an update did not compile the changed files ahead" "$out"
for i in 0 1 2 3 4; do sed -i 's/"v2"/"v3"/' "$dir/parts/Part$i.qml"; done
out=$(start) || fail "a start with five changes did not run the shell" "$out"
[ -z "$(ahead "$out")" ] || fail "five changed files started workers" "$out"
echo "precompile ok"
