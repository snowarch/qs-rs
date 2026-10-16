#!/bin/sh
# Runs "$@" on a private session bus with tests/notifications/mock.py recording server signals.
exec sh tests/lib/private-bus.sh "$(dirname "$0")/mock.py" "$@"
