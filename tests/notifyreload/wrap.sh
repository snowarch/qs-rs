#!/bin/sh
# Same private bus and mock as the notifications test.
exec sh "$(dirname "$0")/../notifications/wrap.sh" "$@"
