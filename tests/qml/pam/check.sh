#!/bin/sh
# pam_exec expose_authtok hands the typed password on stdin.
[ "$(tr -d '\000')" = "hunter2" ]
