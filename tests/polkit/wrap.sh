#!/bin/sh
# Runs "$@" against tests/polkit/mock.py (polkitd, logind and the agent helper socket) on a
# private bus.
export QSRS_POLKIT_HELPER_SOCKET="$XDG_RUNTIME_DIR/polkit-helper.sock"
exec sh tests/lib/private-bus.sh "$(dirname "$0")/mock.py" "$@"
