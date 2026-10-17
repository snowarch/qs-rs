#!/bin/sh
# Same private bus and mock items as the systemtray test.
exec sh "$(dirname "$0")/../systemtray/wrap.sh" "$@"
