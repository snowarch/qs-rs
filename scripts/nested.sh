#!/bin/sh
# Runs a command inside an isolated headless KWin (virtual backend, private D-Bus, private HOME
# and XDG dirs) and saves a screenshot after a delay. Nothing touches the real session.
#   scripts/nested.sh OUT.png DELAY_SECONDS command [args...]
set -u
OUT=$1; DELAY=$2; shift 2
ROOT=${QSRS_NEST_ROOT:-${TMPDIR:-/tmp}/qsrs-nest}
rm -rf "$ROOT"; mkdir -p "$ROOT/run" "$ROOT/home"; chmod 700 "$ROOT/run"
W=${QSRS_NEST_W:-1920}; H=${QSRS_NEST_H:-1080}
export QSRS_OUT=$(realpath -m "$OUT") QSRS_DELAY=$DELAY
exec env -i PATH="$PATH" LANG="${LANG:-C.UTF-8}" HOME="$ROOT/home" XDG_RUNTIME_DIR="$ROOT/run" \
  XDG_CONFIG_HOME="$ROOT/home/.config" XDG_DATA_HOME="$ROOT/home/.local/share" \
  XDG_CACHE_HOME="$ROOT/home/.cache" XDG_STATE_HOME="$ROOT/home/.local/state" \
  XDG_DATA_DIRS=/usr/local/share:/usr/share XDG_CONFIG_DIRS=/etc/xdg \
  KWIN_SCREENSHOT_NO_PERMISSION_CHECKS=1 QSRS_OUT="$QSRS_OUT" QSRS_DELAY="$DELAY" ${QSRS_EXTRA_ENV:-} \
  dbus-run-session -- sh -c '
    kwin_wayland --virtual --width '"$W"' --height '"$H"' --socket wl-qsrs --no-lockscreen >"$XDG_RUNTIME_DIR/kwin.log" 2>&1 &
    KWIN=$!
    for i in $(seq 50); do [ -S "$XDG_RUNTIME_DIR/wl-qsrs" ] && break; sleep 0.1; done
    export WAYLAND_DISPLAY=wl-qsrs QT_QPA_PLATFORM=wayland QT_FORCE_STDERR_LOGGING=1
    "$@" &
    APP=$!
    sleep "$QSRS_DELAY"
    spectacle -b -n -f -o "$QSRS_OUT" >/dev/null 2>&1
    kill $APP 2>/dev/null; wait $APP 2>/dev/null
    kill $KWIN 2>/dev/null; wait $KWIN 2>/dev/null
  ' sh "$@"
