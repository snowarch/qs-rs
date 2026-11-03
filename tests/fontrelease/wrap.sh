# Fonts no live text uses are unmapped after a big teardown in a shown window; fonts in use stay.
# $1 = qs-rs, $2 = qml.
command -v kwin_wayland >/dev/null || { echo "needs a headless KWin (kwin_wayland)"; exit 77; }
nest=$(mktemp -d)
# reclaim is opt-in since 44003e1: the test turns on what it measures
out=$(QSRS_EXTRA_ENV="QSRS_RECLAIM=1" QSRS_NEST_W=800 QSRS_NEST_H=600 QSRS_NEST_ROOT=$nest sh scripts/nested.sh $nest/shot.png 13 "$1" "$2" 2>&1)
rm -rf "$nest"
echo "$out" | grep "FONTS"
shown=$(echo "$out" | grep "FONTS shown:")
dropped=$(echo "$out" | grep "FONTS dropped:")
for f in DejaVuSans DejaVuSerif NotoSerif; do
  case $shown in *$f.ttf*|*$f-Regular.ttf*) ;; *) echo "FAILED: $f should be mapped while shown"; exit 1 ;; esac
done
case $dropped in *DejaVuSans*) ;; *) echo "FAILED: the font still in use was unmapped"; exit 1 ;; esac
case $dropped in *NotoSerif*) echo "FAILED: the dropped texts' font is still mapped"; exit 1 ;; esac
case $dropped in *DejaVuSerif*) echo "FAILED: the dropped native texts' font is still mapped"; exit 1 ;; esac
