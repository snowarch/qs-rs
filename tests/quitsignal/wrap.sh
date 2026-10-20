# SIGTERM and SIGINT end the shell through its exit path: exit 0 within a second, logged.
# $1 = qs-rs, $2 = qml.
bin=$1 qml=$2 fail=0
for sig in TERM INT; do
	out=$(mktemp)
	"$bin" "$qml" >"$out" 2>&1 &
	pid=$!
	i=0; until grep -q READY "$out" || [ $i -ge 100 ]; do sleep 0.1; i=$((i + 1)); done
	kill -$sig $pid
	i=0; while kill -0 $pid 2>/dev/null && [ $i -lt 10 ]; do sleep 0.1; i=$((i + 1)); done
	if kill -0 $pid 2>/dev/null; then echo "FAILED $sig: alive 1 s later"; kill -KILL $pid; fail=1; fi
	wait $pid; rc=$?
	[ $rc -eq 0 ] || { echo "FAILED $sig: exit $rc"; fail=1; }
	grep -q "Quitting on $sig" "$out" || { echo "FAILED $sig: no 'Quitting on $sig'"; fail=1; }
	rm -f "$out"
done
exit $fail
