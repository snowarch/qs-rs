#!/bin/sh
# Runs a command inside niri (winit backend, test config) nested in headless KWin
# (scripts/nested.sh), for protocols KWin lacks (wlr foreign toplevel, ext-session-lock).
# Prints the command's output and saves a grim capture of niri's output after DELAY, then waits
# up to QSRS_NIRI_WAIT seconds (default 6) for the command to exit.
#   scripts/nested-niri.sh OUT.png DELAY command [args...]
set -u
SELF=$(realpath "$0")
case "${1:-}" in
--in-kwin)
  D=$2 O=$3; shift 3
  printf 'hotkey-overlay { skip-at-startup; }\nanimations { off; }\n' >"$XDG_RUNTIME_DIR/niri.kdl"
  exec niri -c "$XDG_RUNTIME_DIR/niri.kdl" -- "$SELF" --in-niri "$D" "$O" "$@" >"$XDG_RUNTIME_DIR/niri.log" 2>&1 ;;
--in-niri)
  D=$2 O=$3; shift 3
  "$@" >"$XDG_RUNTIME_DIR/app.log" 2>&1 &
  APP=$!
  sleep "$D"; grim "$O"
  # Let a self-checking test finish (up to QSRS_NIRI_WAIT seconds) before stopping it.
  (sleep "${QSRS_NIRI_WAIT:-6}"; kill $APP 2>/dev/null) &
  KILLER=$!
  wait $APP
  echo "exit status $?" >>"$XDG_RUNTIME_DIR/app.log"
  kill $KILLER 2>/dev/null
  niri msg action quit -s
  exit ;;
esac
OUT=$(realpath -m "$1"); DELAY=$2; shift 2
ROOT=${QSRS_NEST_ROOT:-${TMPDIR:-/tmp}/qsrs-nest}
QSRS_NEST_W=${QSRS_NEST_W:-1280} QSRS_NEST_H=${QSRS_NEST_H:-800} \
  "$(dirname "$SELF")/nested.sh" "$ROOT-kwin.png" $((DELAY + ${QSRS_NIRI_WAIT:-6} + 4)) "$SELF" --in-kwin "$DELAY" "$OUT" "$@"
cat "$ROOT/run/app.log" 2>/dev/null
