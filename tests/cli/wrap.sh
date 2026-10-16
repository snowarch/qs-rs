#!/bin/sh
# The qs CLI as inir calls it (scripts/inir, setup, dev/tools/nest.sh): daemonize, no-duplicate,
# list text/json, ipc by path and by pid, prop get, kill. Args: <qs-rs> <placeholder qml>.
set -u
qs=$1
dir=$(cd "$(dirname "$0")/shell" && pwd)
fail() { echo "FAIL: $*"; "$qs" -p "$dir" kill >/dev/null 2>&1; exit 1; }

"$qs" -p "$dir" list | grep -q '^No running instances for' || fail "list before launch"
# -d returns once the config is loaded, with the shell already answering.
"$qs" -n -d -p "$dir" || fail "daemonize exit code"
pid=$("$qs" -p "$dir" list | awk '/Process ID:/{print $3}')
[ -n "$pid" ] && kill -0 "$pid" || fail "no instance after -d"
[ "$(ps -o sid= -p "$pid" | tr -d ' ')" = "$pid" ] || fail "daemon is not a session leader"
[ "$("$qs" -p "$dir" ipc call cli echo hi)" = "echo:hi" ] || fail "ipc call by path"
[ "$("$qs" ipc --pid "$pid" call cli echo x)" = "echo:x" ] || fail "ipc --pid call"
[ "$("$qs" ipc --pid "$pid" prop get cli mode)" = "ready" ] || fail "prop get"
"$qs" ipc --pid "$pid" show | grep -q '^target cli' || fail "ipc show"
"$qs" -p "$dir" ipc call nope f 2>/dev/null && fail "unknown target must exit nonzero"
# -n with a live instance exits 0 without a second one.
"$qs" -n -d -p "$dir" | grep -q 'already running' || fail "no-duplicate message"
[ "$("$qs" list --all | grep -c 'Process ID:')" = 1 ] || fail "duplicate started"
"$qs" list --all --json | grep -q "\"pid\": $pid" || fail "list json"
"$qs" -p "$dir" list | grep -q "Config path: $dir/shell.qml" || fail "list config path"
"$qs" -p "$dir" kill | grep -q '^Killed ' || fail "kill message"
# Gone or a zombie waiting for its new parent to reap it.
state=$(ps -o stat= -p "$pid" 2>/dev/null | cut -c1)
[ -z "$state" ] || [ "$state" = Z ] || fail "instance alive after kill (state $state)"
"$qs" -p "$dir" kill >/dev/null && fail "kill with nothing running must fail"
"$qs" -p "$dir" list | grep -q '^No running instances for' || fail "list after kill"
echo "cli ok"
