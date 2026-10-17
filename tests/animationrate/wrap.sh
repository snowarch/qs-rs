#!/bin/sh
# A 120 Hz stand-in screen for the animation driver (offscreen has no refresh rate of its own).
QSRS_ANIMATION_HZ=120 exec "$@"
