#!/bin/sh
# Runs a command inside a real niri (winit backend, with blur) nested in a headless KWin
# (virtual backend, private D-Bus, HOME and XDG dirs). For what only niri does or enforces.
#   scripts/niri-nested.sh SECONDS command [args...]
# The command runs as niri's client for SECONDS, then everything is stopped.
set -u
SECS=$1; shift
ROOT=${QSRS_NEST_ROOT:-${TMPDIR:-/tmp}/qsrs-niri}
rm -rf "$ROOT"; mkdir -p "$ROOT/run" "$ROOT/home/.config"; chmod 700 "$ROOT/run"
W=${QSRS_NEST_W:-1280}; H=${QSRS_NEST_H:-800}
printf '[org.kde.kdecoration2]\nNoPlugin=true\n' > "$ROOT/home/.config/kwinrc"
{
  printf 'output "winit" { mode "%sx%s"; }\nhotkey-overlay { skip-at-startup; }\nprefer-no-csd\n' "$W" "$H"
  printf 'spawn-at-startup "sh" "%s/inside.sh"\n' "$ROOT"
  printf 'blur {\n    passes 2\n    offset 3.0\n}\n'
} > "$ROOT/niri.kdl"
# The command's arguments travel through a file: niri's spawn takes a fixed argv.
for a in "$@"; do printf '%s\0' "$a"; done > "$ROOT/argv"
cat > "$ROOT/inside.sh" <<IN
#!/bin/sh
export QT_QPA_PLATFORM=wayland QT_FORCE_STDERR_LOGGING=1
xargs -0 sh -c '"\$@" & echo \$! > "$ROOT/app.pid"; wait' sh < "$ROOT/argv" > "$ROOT/app.log" 2>&1
IN
exec env -i PATH="$PATH" LANG="${LANG:-C.UTF-8}" HOME="$ROOT/home" XDG_RUNTIME_DIR="$ROOT/run" \
  XDG_CONFIG_HOME="$ROOT/home/.config" XDG_DATA_HOME="$ROOT/home/.local/share" \
  XDG_CACHE_HOME="$ROOT/home/.cache" XDG_STATE_HOME="$ROOT/home/.local/state" \
  XDG_DATA_DIRS=/usr/local/share:/usr/share XDG_CONFIG_DIRS=/etc/xdg KWIN_COMPOSE=O2 ${QSRS_EXTRA_ENV:-} \
  dbus-run-session -- sh -c '
    kwin_wayland --virtual --width '"$W"' --height '"$H"' --socket host --no-lockscreen --no-global-shortcuts >"$1/kwin.log" 2>&1 &
    KWIN=$!
    for i in $(seq 50); do [ -S "$XDG_RUNTIME_DIR/host" ] && break; sleep 0.1; done
    WAYLAND_DISPLAY=host niri -c "$1/niri.kdl" >"$1/niri.log" 2>&1 &
    NIRI=$!
    sleep "$2"
    [ -f "$1/app.pid" ] && kill $(cat "$1/app.pid") 2>/dev/null
    sleep 0.3
    kill $NIRI 2>/dev/null; wait $NIRI 2>/dev/null
    kill $KWIN 2>/dev/null; wait $KWIN 2>/dev/null
  ' sh "$ROOT" "$SECS"
