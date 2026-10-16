#!/bin/sh
# Build qs-rs and run every tests/qml/*.qml (or the given files) in an isolated env.
# A test passes when it calls Qt.exit(0) within the timeout.
set -u
cd "$(dirname "$0")/.."
cargo build -q || exit 1
BIN=$(cargo metadata -q --format-version 1 | sed 's/.*"target_directory":"\([^"]*\)".*/\1/')/debug/qs-rs
export QSRS_ISO_ROOT=${QSRS_ISO_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/qs-rs/iso-test}
export QSRS_RUNTIME=${QSRS_RUNTIME:-${TMPDIR:-/tmp}/qsrs-run}
fail=0
for f in ${@:-tests/qml/*.qml}; do
  rm -rf "$QSRS_ISO_ROOT" "$QSRS_RUNTIME"; mkdir -p "$QSRS_RUNTIME"; chmod 700 "$QSRS_RUNTIME"
  out=$(timeout "${QSRS_TIMEOUT:-10}" scripts/iso.sh "$BIN" "$f" 2>&1); rc=$?
  if [ $rc -eq 0 ]; then echo "PASS $f"; else echo "FAIL $f (exit $rc)"; echo "$out" | tail -20 | sed 's/^/    /'; fail=1; fi
done
exit $fail
