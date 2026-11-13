#!/bin/sh
# Runs "$@" on a private session bus with two tray items, one whose menu never answers a property read.
exec sh tests/lib/private-bus.sh tests/dbusmenustall/mock.py "$@"
