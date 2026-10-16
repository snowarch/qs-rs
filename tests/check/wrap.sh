#!/bin/sh
# `qs-rs check` on a fixture shell whose broken area is never loaded: the error must be reported
# with its file and line, and the exit code must say so. Args: <qs-rs> <placeholder qml>.
out=$("$1" -p "$(dirname "$0")/shell/shell.qml" check 2>/dev/null); rc=$?
echo "$out"
[ $rc -eq 1 ] || exit 1
echo "$out" | grep -qx "areas/Broken.qml:3:2: NoSuchType is not a type" || exit 1
echo "$out" | grep -qx "3 files checked, 2 errors"
