#!/bin/sh
# Build qs-rs and run every tests/qml/*.qml (or the given files) in an isolated env.
# A test passes when it calls Qt.exit(0) within the timeout. Exit 77 (from its wrap.sh) skips it: what it needs, a
# compositor or a system service, is not on this machine (CI); the line says what.
set -u
cd "$(dirname "$0")/.."
# QSRS_BIN: a binary already built (CI's bundle, bin/qs); otherwise a debug build here.
if [ -n "${QSRS_BIN:-}" ]; then BIN=$QSRS_BIN; else
  cargo build -q || exit 1
  BIN=$(cargo metadata -q --format-version 1 | sed 's/.*"target_directory":"\([^"]*\)".*/\1/')/debug/qs-rs
fi
export QSRS_ISO_ROOT=${QSRS_ISO_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/qs-rs/iso-test}
export QSRS_RUNTIME=${QSRS_RUNTIME:-${TMPDIR:-/tmp}/qsrs-run}
fail=0
for f in ${@:-tests/qml/*.qml}; do
  rm -rf "$QSRS_ISO_ROOT" "$QSRS_RUNTIME"; mkdir -p "$QSRS_RUNTIME"; chmod 700 "$QSRS_RUNTIME"
  # tests/<name>/wrap.sh, when present, wraps the run (e.g. a private daemon the test talks to)
  wrap=tests/$(basename "$f" .qml)/wrap.sh; [ -f "$wrap" ] || wrap=
  out=$(timeout "${QSRS_TIMEOUT:-20}" scripts/iso.sh ${wrap:+sh "$wrap"} "$BIN" "$f" 2>&1); rc=$?
  if [ $rc -eq 0 ]; then echo "PASS $f"
  elif [ $rc -eq 77 ]; then echo "SKIP $f: $(echo "$out" | tail -1)"
  else echo "FAIL $f (exit $rc)"; echo "$out" | tail -20 | sed 's/^/    /'; fail=1; fi
done
exit $fail
