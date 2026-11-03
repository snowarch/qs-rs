#!/bin/sh
# Runs "$@" against a private PipeWire daemon inside the isolated XDG_RUNTIME_DIR.
command -v pipewire >/dev/null || { echo "needs a PipeWire daemon (pipewire)"; exit 77; }
conf=$(realpath "$(dirname "$0")/pipewire.conf")
pipewire -c "$conf" >"$XDG_RUNTIME_DIR/pipewire.log" 2>&1 &
pw=$!
trap 'kill $pw 2>/dev/null' EXIT
i=0
while [ ! -S "$XDG_RUNTIME_DIR/pipewire-0" ] && [ $i -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
# Without a session manager nobody configures ports; give both nodes stereo dsp ports.
for n in "qsrs-test-sink Input" "qsrs-test-source Output"; do
  set -- $n "$@"
  pw-cli s "$1" PortConfig "{ direction: $2, mode: dsp, format: { mediaType: audio, mediaSubtype: raw,
    format: F32P, rate: 48000, channels: 2, position: [ FL, FR ] } }" >/dev/null
  shift 2
done
"$@"
