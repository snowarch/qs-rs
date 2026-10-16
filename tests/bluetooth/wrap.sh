#!/bin/sh
# Runs "$@" against tests/bluetooth/mock.py on a private bus.
exec sh tests/lib/private-bus.sh "$(dirname "$0")/mock.py" "$@"
