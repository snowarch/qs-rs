#!/bin/sh
# Same mock polkitd as the polkit test.
exec sh "$(dirname "$0")/../polkit/wrap.sh" "$@"
