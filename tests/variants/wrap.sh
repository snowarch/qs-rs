#!/bin/sh
# Runs the test and fails if Qt reported a Variants instance outside the graphics scene.
out=$("$@" 2>&1); rc=$?
echo "$out"
[ $rc -eq 0 ] || exit $rc
! echo "$out" | grep -q "not placed in the graphics scene"
