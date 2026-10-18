#!/bin/sh
# Quickshell.watchFiles: saving a file the shell loaded reloads it once. Same content, the empty
# half of a two-step save, a second save of a replaced file (new inode), a burst over three files
# and watchFiles turned off are each checked by counting reloads. Args: <qs-rs> <placeholder>.
dir=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/watch.XXXXXX")
child() { printf 'import QtQuick\nQtObject { property bool watch: %s; property int v: %s }\n' "$1" "$2"; }
printf '//@ pragma ShellId watchtest\nimport QtQuick\nimport Quickshell\nShellRoot {\n    Child { id: c }\n    Other {}\n    Third {}\n    Binding { target: Quickshell; property: "watchFiles"; value: c.watch }\n}\n' >"$dir/shell.qml"
child true 1 >"$dir/Child.qml"
for f in Other Third; do printf 'import QtQuick\nQtObject { property int v: 1 }\n' >"$dir/$f.qml"; done
log=$dir/log
"$1" -p "$dir/shell.qml" >"$log" 2>&1 &
pid=$!
trap 'kill $pid 2>/dev/null; rm -rf "$dir"' EXIT
reloads() { grep -c "Reloading configuration" "$log"; }
expect() { sleep 0.5; n=$(reloads); [ "$n" -eq "$1" ] || { echo "FAILED $2: $n reloads, want $1"; cat "$log"; exit 1; }; }
i=0; until grep -q "Configuration Loaded" "$log"; do i=$((i+1)); [ $i -gt 100 ] && { cat "$log"; exit 1; }; sleep 0.05; done
child true 1 >"$dir/Child.qml"; expect 0 "same content"
: >"$dir/Child.qml"; expect 0 "empty truncate"
child true 2 >"$dir/Child.qml"; expect 1 "in-place save"
child true 3 >"$dir/new"; mv "$dir/new" "$dir/Child.qml"; expect 2 "replace"
child true 4 >"$dir/new"; mv "$dir/new" "$dir/Child.qml"; expect 3 "second replace"
for f in Other Third; do printf 'import QtQuick\nQtObject { property int v: 2 }\n' >"$dir/$f.qml"; done
child true 5 >"$dir/Child.qml"; expect 4 "burst of three"
child false 6 >"$dir/Child.qml"; expect 5 "turning it off"
child false 7 >"$dir/Child.qml"; expect 5 "while off"
