#!/bin/sh
# The mock polkitd turns the first registration away, as when a nested test shell got there first.
export QSRS_MOCK_POLKIT_BUSY=1
exec sh "$(dirname "$0")/../polkit/wrap.sh" "$@"
