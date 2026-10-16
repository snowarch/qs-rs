#!/bin/sh
# Runs "$@" with a private bus as the system bus, serving tests/upower/mock.py.
dir=$(realpath "$(dirname "$0")")
addr=$(dbus-daemon --session --fork --print-address=1 --print-pid=3 --nopidfile 3>"$XDG_RUNTIME_DIR/bus.pid")
export DBUS_SYSTEM_BUS_ADDRESS=$addr
python3 "$dir/mock.py" >"$XDG_RUNTIME_DIR/mock.log" 2>&1 &
mock=$!
trap 'kill $mock $(cat "$XDG_RUNTIME_DIR/bus.pid") 2>/dev/null' EXIT
i=0
until gdbus call --system -d org.qsrs.Mock -o /org/qsrs/Mock -m org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1 || [ $i -ge 50 ]; do
  sleep 0.1; i=$((i + 1))
done
"$@"
