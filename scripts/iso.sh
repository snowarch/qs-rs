#!/bin/sh
# Run a command with an isolated HOME and XDG dirs; nothing leaks into the real session.
set -eu
ROOT=${QSRS_ISO_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/qs-rs/iso}
mkdir -p "$ROOT/home" "$ROOT/config" "$ROOT/data" "$ROOT/cache" "$ROOT/state" "$ROOT/run"
chmod 700 "$ROOT/run"
exec env -i \
  PATH="$PATH" LANG="${LANG:-C.UTF-8}" \
  HOME="$ROOT/home" XDG_CONFIG_HOME="$ROOT/config" XDG_DATA_HOME="$ROOT/data" \
  XDG_CACHE_HOME="$ROOT/cache" XDG_STATE_HOME="$ROOT/state" XDG_RUNTIME_DIR="${QSRS_RUNTIME:-$ROOT/run}" \
  XDG_DATA_DIRS=/usr/local/share:/usr/share XDG_CONFIG_DIRS=/etc/xdg \
  QT_FORCE_STDERR_LOGGING=1 QT_LOGGING_RULES="${QSRS_LOG_RULES:-default.debug=true;js.debug=true;qt.*.debug=false}" QT_QPA_PLATFORM="${QSRS_QPA:-offscreen}" ${QSRS_WAYLAND:+WAYLAND_DISPLAY=$QSRS_WAYLAND} \
  ${QSRS_EXTRA_ENV:-} "$@"
