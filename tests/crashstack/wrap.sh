# Fatal signal handler: the QML stack shows up (or a clear note), and the process always dies
# by the signal instead of faulting silently or hanging inside the handler. $1 = qs-rs, $2 = qml.
bin=$1 qml=$2 fail=0
ulimit -c 0 # the crashes are on purpose: no cores
run() { # case, expected text, kill after seconds (0 = crashes by itself)
	rm -f "$XDG_STATE_HOME"/quickshell/*/crash-generation "$XDG_STATE_HOME"/qs-rs/*/crash-generation 2>/dev/null
	find "$XDG_STATE_HOME" -name crash-generation -delete 2>/dev/null
	out=$(mktemp)
	if [ "$1" = teardown ]; then
		CRASH_CASE=$1 QSRS_TEST_TEARDOWN_JS=1 "$bin" "$qml" >"$out" 2>&1 &
	else
		CRASH_CASE=$1 "$bin" "$qml" >"$out" 2>&1 &
	fi
	pid=$!
	if [ "$3" != 0 ]; then
		i=0; until grep -q "$(echo "$1" | tr a-z A-Z)ING\|IDLE" "$out" || [ $i -ge 100 ]; do sleep 0.1; i=$((i + 1)); done
		kill -SEGV $pid
	fi
	i=0; while kill -0 $pid 2>/dev/null && [ $i -lt 100 ]; do sleep 0.1; i=$((i + 1)); done
	if kill -0 $pid 2>/dev/null; then echo "FAILED $1: still alive 10 s later"; kill -KILL $pid; fail=1; fi
	wait $pid; rc=$?
	if [ $rc -ne 139 ]; then echo "FAILED $1: exit $rc, not SIGSEGV"; fail=1; fi
	if ! sed -n '/QML stack/,$p' "$out" | grep -q "$2"; then
		echo "FAILED $1: no '$2' after the QML stack header"; sed -n '/fatal signal/,$p' "$out" | grep -v '^/' | head -20; fail=1
	fi
	rm -f "$out"
}
run spin 'spin  .*crashstack.qml:[0-9]' 1
run idle '(engine not running JS)' 1
run teardown 'Probe.qml:[0-9]' 0
exit $fail
