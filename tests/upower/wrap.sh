#!/bin/sh
# Runs "$@" against tests/upower/mock.py on a private bus.
exec sh tests/lib/private-bus.sh "$(dirname "$0")/mock.py" "$@"
