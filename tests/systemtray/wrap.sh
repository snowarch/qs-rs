#!/bin/sh
# Runs "$@" on a private session bus with tests/systemtray/mock.py serving two tray items.
exec sh tests/lib/private-bus.sh "$(dirname "$0")/mock.py" "$@"
