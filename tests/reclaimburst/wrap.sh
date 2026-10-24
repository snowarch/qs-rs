# A teardown in a shown window reclaims memory once it settles; a small one does not.
# $1 = qs-rs, $2 = qml.
# reclaim is opt-in since 44003e1: the test turns on what it measures
out=$(QSRS_RECLAIM=1 QSRS_RECLAIM_LOG=1 "$1" "$2" 2>&1)
echo "$out" | grep -E "DROPPED|qsrs-memory"
small=$(echo "$out" | sed -n '/SMALL DROPPED/,/BIG DROPPED/p' | grep -c "qsrs-memory burst")
big=$(echo "$out" | sed -n '/BIG DROPPED/,$p' | grep -c "qsrs-memory burst")
[ "$small" -eq 0 ] || { echo "FAILED: a 300-object drop reclaimed"; exit 1; }
[ "$big" -eq 1 ] || { echo "FAILED: the 6000-object drop reclaimed $big times"; exit 1; }
