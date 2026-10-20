#!/bin/sh
# The systemtray mock, and a failure on any warning from a menu entry.
out=$(sh "$(dirname "$0")/../systemtray/wrap.sh" "$@" 2>&1); rc=$?
echo "$out"
if echo "$out" | grep -q "DBusMenuItem.qml"; then echo "FAILED: $(echo "$out" | grep -c 'DBusMenuItem.qml') warnings from menu entries"; exit 1; fi
exit $rc
