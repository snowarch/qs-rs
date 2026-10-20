#!/bin/sh
# Runs "$@" on a private session bus with the tray mock (which the test kills mid-way).
exec sh tests/lib/private-bus.sh tests/systemtray/mock.py "$@"
