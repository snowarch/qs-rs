#!/bin/sh
# Runs "$@" against tests/upower/mock.py on a private system bus.
exec sh tests/lib/system-bus.sh "$(dirname "$0")/mock.py" "$@"
