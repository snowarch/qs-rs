#!/bin/sh
# Apples-to-apples A/B benchmark in an isolated headless KWin nest: compares an existing `qs` binary
# (default: ~/.local/share/inir/runtime/current/bin/qs) against a candidate (default: local release
# target/release/qs-rs). Pass `--same-libs` when comparing two binaries built against the same Qt.
#
#   scripts/bench-local.sh [OLD_BIN] [NEW_BIN]
set -eu
REPO=$(cd "$(dirname "$0")/.." && pwd)
OLD=${1:-${XDG_DATA_HOME:-$HOME/.local/share}/inir/runtime/current/bin/qs}
TARGET=$(cargo metadata --manifest-path "$REPO/Cargo.toml" -q --format-version 1 | sed 's/.*"target_directory":"\([^"]*\)".*/\1/')
NEW=${2:-$TARGET/release/qs-rs}
SHELL_QML=${INIR_BENCH_SHELL:-$REPO/../inir/shell.qml}
RUNS=${INIR_BENCH_RUNS:-3}

[ -x "$OLD" ] || { echo "missing OLD binary: $OLD" >&2; exit 1; }
[ -x "$NEW" ] || { echo "missing NEW binary: $NEW (run cargo build --release)" >&2; exit 1; }
[ -f "$SHELL_QML" ] || { echo "missing shell.qml: $SHELL_QML" >&2; exit 1; }

echo "OLD: $OLD"
echo "NEW: $NEW"
echo "SHELL: $SHELL_QML ($RUNS warm runs each)"

# FieldRegion micro-benchmark (10 builds, median µs)
for pair in "OLD:$OLD" "NEW:$NEW"; do
  label=${pair%%:*}; bin=${pair#*:}
  us=$("$bin" -p "$REPO/tests/qml/fieldregion.qml" 2>&1 | sed -n 's/.*"micros":\([0-9]*\).*/\1/p' | tail -n1)
  echo "FieldRegion ($label): ${us:-?} us"
done

ROOT=$(mktemp -d "${TMPDIR:-/tmp}/qsrs-bench.XXXXXX")
trap 'rm -rf "$ROOT"' EXIT
mkdir -p "$ROOT/run" "$ROOT/home" "$ROOT/cache"
chmod 700 "$ROOT/run"

env -i PATH="$PATH" LANG="${LANG:-C.UTF-8}" HOME="$ROOT/home" XDG_RUNTIME_DIR="$ROOT/run" \
  XDG_CONFIG_HOME="$ROOT/home/.config" XDG_DATA_HOME="$ROOT/home/.local/share" \
  XDG_CACHE_HOME="$ROOT/cache" XDG_STATE_HOME="$ROOT/home/.local/state" \
  XDG_DATA_DIRS=/usr/local/share:/usr/share XDG_CONFIG_DIRS=/etc/xdg \
  dbus-run-session -- sh -c '
    OLD=$1; NEW=$2; SHELL_QML=$3; RUNS=$4
    kwin_wayland --virtual --output-count 1 --width 1920 --height 1080 --socket wl-bench --no-lockscreen >"$XDG_RUNTIME_DIR/kwin.log" 2>&1 &
    KWIN=$!
    for i in $(seq 50); do [ -S "$XDG_RUNTIME_DIR/wl-bench" ] && break; sleep 0.1; done
    export WAYLAND_DISPLAY=wl-bench QT_QPA_PLATFORM=wayland
    for pair in "OLD:$OLD" "NEW:$NEW"; do
      label=${pair%%:*}; bin=${pair#*:}
      echo "--- $label ---"
      for run in $(seq 0 "$RUNS"); do
        log="$XDG_RUNTIME_DIR/${label}_${run}.log"
        "$bin" -v -p "$SHELL_QML" >"$log" 2>&1 &
        APP=$!
        i=0
        until grep -q "Configuration loaded in" "$log" 2>/dev/null || [ "$i" -ge 150 ]; do
          i=$((i + 1)); sleep 0.1
        done
        sleep 3
        if [ "$run" -gt 0 ]; then
          load_ms=$(sed -n "s/.*Configuration loaded in \([0-9]*\) ms.*/\1/p" "$log" | tail -n1)
          threads=$(ls "/proc/$APP/task" 2>/dev/null | wc -l)
          mem=$("$bin" memory --pid "$APP" 2>/dev/null | head -n1)
          rss=$(echo "$mem" | sed -n "s/.*rss=\([0-9]*MB\).*/\1/p")
          alloc=$(echo "$mem" | sed -n "s/.*allocated=\([0-9]*MB\).*/\1/p")
          res=$(echo "$mem" | sed -n "s/.*resident=\([0-9]*MB\).*/\1/p")
          printf "  run %d: load=%sms  rss=%s  alloc=%s  resident=%s  threads=%s\n" "$run" "${load_ms:-?}" "${rss:-?}" "${alloc:-?}" "${res:-?}" "$threads"
        fi
        kill -TERM "$APP" 2>/dev/null || true
        wait "$APP" 2>/dev/null || true
      done
    done
    kill "$KWIN" 2>/dev/null || true
    wait "$KWIN" 2>/dev/null || true
  ' sh "$OLD" "$NEW" "$SHELL_QML" "$RUNS"
