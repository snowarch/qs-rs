#!/bin/sh
# A shell that quits right after compiling (inside the cache sync's 1.5 s quiet time) leaves no `.dirty` marker, so
# its next start keeps the cache instead of compiling everything again. Args: <qs-rs> <placeholder qml>.
set -u
dir=$XDG_STATE_HOME/cacheflush-shell
mkdir -p "$dir"
printf 'import QtQuick\nimport Quickshell\nShellRoot {\n    Item { property int n: 1 }\n    Component.onCompleted: Qt.exit(0)\n}\n' > "$dir/shell.qml"
cache=${XDG_CACHE_HOME:-$HOME/.cache}/quickshell/qmlcache
NO_COLOR=1 timeout 15 "$1" -v -p "$dir/shell.qml" >/dev/null 2>&1 || { echo "FAIL: the shell did not run"; exit 1; }
[ -n "$(ls "$cache" 2>/dev/null)" ] || { echo "FAIL: nothing was compiled into $cache"; exit 1; }
[ ! -e "$cache/.dirty" ] || { echo "FAIL: a clean exit left the cache marked dirty"; exit 1; }
out=$(NO_COLOR=1 timeout 15 "$1" -v -p "$dir/shell.qml" 2>&1) || { echo "FAIL: the second start did not run"; exit 1; }
case $out in *"was not flushed"*) echo "FAIL: the second start dropped the cache"; exit 1 ;; esac
echo "cacheflush ok"
