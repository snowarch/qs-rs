#!/bin/sh
# Runs "$@" on a private session bus with the tray mock, whose "pathonly" item has a dbusmenu.
exec sh tests/lib/private-bus.sh tests/systemtray/mock.py "$@"
