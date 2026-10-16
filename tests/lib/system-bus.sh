#!/bin/sh
# system-bus.sh MOCK.py CMD...: runs CMD with a private bus as the system bus, serving MOCK.py
# (which must own org.qsrs.Mock once it is ready).
mockpy=$(realpath "$1"); shift
addr=$(dbus-daemon --session --fork --print-address=1 --print-pid=3 --nopidfile 3>"$XDG_RUNTIME_DIR/bus.pid")
export DBUS_SYSTEM_BUS_ADDRESS=$addr
python3 "$mockpy" >"$XDG_RUNTIME_DIR/mock.log" 2>&1 &
mock=$!
trap 'kill $mock $(cat "$XDG_RUNTIME_DIR/bus.pid") 2>/dev/null' EXIT
i=0
until gdbus call --system -d org.qsrs.Mock -o /org/qsrs/Mock -m org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1 || [ $i -ge 50 ]; do
  sleep 0.1; i=$((i + 1))
done
"$@"
