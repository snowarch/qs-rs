# The memory peak log: one startup line, then a peak line naming the window that was shown.
# $1 = qs-rs, $2 = qml.
bin=$1 qml=$2
log=$XDG_STATE_HOME/qs-rs/memory-peaks
rm -f "$log"  # the state dir outlives a run: only this one's lines count
"$bin" "$qml" >/dev/null 2>&1 &
pid=$!
i=0; until grep -q "new peak" "$log" 2>/dev/null || [ $i -ge 150 ]; do sleep 0.1; i=$((i + 1)); done
kill -TERM $pid; wait $pid
cat "$log" 2>/dev/null
grep -q "startup peak" "$log" && grep "new peak" "$log" | grep -q "show peaks.qml" || { echo "FAILED: no peak naming peaks.qml"; exit 1; }
