#!/bin/sh
# The test reads the system's NetworkManager (read-only); a machine without one has nothing to read.
busctl --system status org.freedesktop.NetworkManager >/dev/null 2>&1 || { echo "needs NetworkManager on the system bus"; exit 77; }
exec "$@"
