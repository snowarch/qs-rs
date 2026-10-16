#!/bin/sh
# Runs "$@" with XDG_DATA_HOME and XDG_DATA_DIRS on fresh desktop entry fixtures.
set -eu
D=$(mktemp -d "$XDG_RUNTIME_DIR/desktopentries.XXXXXX")
trap 'rm -rf "$D"' EXIT
mkdir -p "$D/home/applications" "$D/sys/applications/kde"
entry() { # file name [extra lines...]
  f=$1 n=$2; shift 2
  printf '[Desktop Entry]\nType=Application\nName=%s\nExec=true\n' "$n" >"$f"
  for l in "$@"; do printf '%s\n' "$l" >>"$f"; done
}
entry "$D/sys/applications/foo.desktop" "Foo sys" "StartupWMClass=FooWin"
entry "$D/sys/applications/hideme.desktop" "HideMe"
entry "$D/sys/applications/kde/sub.desktop" "Sub"
entry "$D/sys/applications/nodisp.desktop" "NoDisp" "NoDisplay=true"
printf '[Desktop Entry]\nType=Link\nName=Link\nURL=x\n' >"$D/sys/applications/link.desktop"
entry "$D/home/applications/foo.desktop" "Foo home" "Name[zz]=Wrong" "StartupWMClass=FooWin" \
  "Exec=sh -c \"touch \$0\" $D/ran %U" "Path=$D" "Terminal=true" "Categories=A;B;" "Keywords=k1;" \
  "Actions=second;first;" "" "[Desktop Action first]" "Name=First" "Exec=touch $D/first" \
  "[Desktop Action second]" "Name=Second" "Icon=sec"
printf '[Desktop Entry]\nType=Application\nHidden=true\n' >"$D/home/applications/hideme.desktop"
entry "$D/home/applications/Mixed.Case.desktop" "Mixed"
XDG_DATA_HOME=$D/home XDG_DATA_DIRS=$D/sys QSRS_TEST_DIR=$D "$@"
