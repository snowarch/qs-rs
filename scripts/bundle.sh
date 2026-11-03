#!/usr/bin/env bash
# qs-rs with its own Qt, as one folder people unpack: bin/qs, lib/, plugins/, qml/, bin/qt.conf. qs-rs uses Qt
# private APIs, so a build against a distro's Qt breaks on that distro's next Qt update; this one never meets it.
# Mesa, EGL, wayland, glibc, pam, pipewire, dbus, fontconfig stay the system's.
#
#   scripts/bundle.sh QT_DIR OUT_DIR
#     QT_DIR   an official Qt build, e.g. aqt's <root>/6.12.0/linux_gcc_64 (private headers included)
#     OUT_DIR  gets qs-rs-<version>-x86_64.tar.gz and its .sha256 (build trees in OUT_DIR/.work); when QT_DIR has
#              Qt WebEngine, also qs-rs-webengine-<version>-x86_64.tar.gz, unpacked over the first one
#
# Needs: cargo, cmake >= 3.30, meson, ninja, git, patchelf, jq, clang, lld, a C++ compiler, wayland-scanner,
# wayland-protocols, and the headers of pam, pipewire, jemalloc, xkbcommon, wayland, ffi, expat. Build on the oldest
# glibc and libwayland people run (ubuntu-24.04): what links here runs there and on anything newer.
set -euo pipefail

QT=$(realpath "${1:?QT_DIR}")
OUT=$(realpath -m "${2:?OUT_DIR}")
ROOT=$(cd "$(dirname "$0")/.." && pwd)
KF=v6.31.0       # Kirigami, syntax-highlighting, extra-cmake-modules
PLASMA=v6.8.1    # layer-shell-qt
WAYLAND=1.24.0   # headers only, see below
WORK=$OUT/.work
STAGE=$WORK/stage
VER=$(cargo metadata --no-deps --format-version 1 --manifest-path "$ROOT/Cargo.toml" |
    jq -r '.packages[] | select(.name == "qs-rs") | .version')
NAME=qs-rs-$VER-x86_64
DEST=$OUT/$NAME
mkdir -p "$WORK" "$STAGE/include"

quiet() { "$@" >"$WORK/step.log" 2>&1 || { tail -60 "$WORK/step.log"; return 1; }; }
clone() { [ -d "$3" ] || git -c advice.detachedHead=false clone -q --depth 1 --branch "$2" "$1" "$3"; }

# Qt 6.12's private wayland headers come from wayland 1.24's protocol (wl_fixes); older distros' headers lack it.
# Only the headers move ahead: everything still links the host's libwayland, so a newer symbol fails the link here.
if [ ! -f "$STAGE/include/wayland-client-protocol.h" ]; then
    clone https://gitlab.freedesktop.org/wayland/wayland.git "$WAYLAND" "$WORK/wayland"
    quiet meson setup "$WORK/wayland/build" "$WORK/wayland" --prefix="$WORK/wayland/prefix" \
        -Ddocumentation=false -Dtests=false -Ddtd_validation=false
    quiet meson install -C "$WORK/wayland/build"
    cp "$WORK/wayland/prefix/include"/wayland-*.h "$STAGE/include/"
fi
export CPLUS_INCLUDE_PATH="$STAGE/include${CPLUS_INCLUDE_PATH:+:$CPLUS_INCLUDE_PATH}"

# KDE parts the shell imports, built against this Qt into the stage
kde() {
    local repo=$1 tag=$2 src=$WORK/${1##*/}
    shift 2
    clone "https://invent.kde.org/$repo.git" "$tag" "$src"
    quiet cmake -S "$src" -B "$src/build" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$STAGE" \
        -DCMAKE_PREFIX_PATH="$QT;$STAGE" -DBUILD_TESTING=OFF -DBUILD_QCH=OFF -DBUILD_PYTHON_BINDINGS=OFF \
        -DKDE_INSTALL_LIBDIR=lib -DKDE_INSTALL_QMLDIR=qml -DKDE_INSTALL_QTPLUGINDIR=plugins "$@"
    quiet cmake --build "$src/build"
    quiet cmake --install "$src/build"
}
kde frameworks/extra-cmake-modules "$KF"
kde plasma/layer-shell-qt "$PLASMA"
kde frameworks/kirigami "$KF"
kde frameworks/syntax-highlighting "$KF"

QMAKE=$QT/bin/qmake \
    LIBRARY_PATH="$STAGE/lib${LIBRARY_PATH:+:$LIBRARY_PATH}" CARGO_TARGET_DIR="$WORK/target" \
    cargo build --release --locked --manifest-path "$ROOT/Cargo.toml" -p qs-rs

rm -rf "$DEST"
mkdir -p "$DEST/bin" "$DEST/lib" "$DEST/plugins" "$DEST/qml"
cp "$WORK/target/release/qs-rs" "$DEST/bin/qs"
printf '[Paths]\nPrefix = ..\nLibraries = lib\nPlugins = plugins\nQmlImports = qml\n' >"$DEST/bin/qt.conf"

# what the shell loads at runtime (qs-rs's own Quickshell modules are linked into the binary)
for dir in platforms wayland-shell-integration wayland-graphics-integration-client wayland-decoration-client \
    imageformats iconengines tls multimedia platforminputcontexts; do
    for from in "$QT/plugins/$dir" "$STAGE/plugins/$dir"; do
        [ -d "$from" ] && mkdir -p "$DEST/plugins/$dir" && cp -a "$from/." "$DEST/plugins/$dir/"
    done
done
# wayland as a client, plus offscreen for what runs without a screen (`check`, the precompile workers a cold start
# runs); the other platforms, shells and the compositor-side buffer plugins never load
rm -f "$DEST"/plugins/platforms/libq{minimal,vnc,linuxfb,eglfs,xcb,vkkhrdisplay}*.so \
    "$DEST"/plugins/wayland-shell-integration/lib{fullscreen-shell-v1,ivi-shell,qt-shell,wl-shell-plugin}.so \
    "$DEST"/plugins/wayland-graphics-integration-client/*-server.so
for mod in QtQuick QtQml QtCore Qt5Compat QtMultimedia Qt/labs/folderlistmodel; do
    mkdir -p "$DEST/qml/$mod" && cp -a "$QT/qml/$mod/." "$DEST/qml/$mod/"
done
# what no shipped QML imports (and the libraries only they pull in)
rm -rf "$DEST"/qml/QtQuick/{Particles,VectorImage,LocalStorage,tooling,Shapes/DesignHelpers} \
    "$DEST"/qml/QtQuick/Controls/{Imagine,Universal} "$DEST"/qml/QtQml/{XmlListModel,DesignSupport}
find "$DEST/qml" -type d -name designer -prune -exec rm -rf {} +
cp -a "$STAGE/qml/org" "$DEST/qml/"

# what runs: the shell, Chromium's helper process, every library and plugin
elves() { find "$1" -type f \( -name qs -o -name QtWebEngineProcess -o -name '*.so*' \) "${@:2}"; }
needs() { ldd "$1" 2>/dev/null | awk '$3 ~ /^\// {print $3}'; }
# carry ROOT [UNDER]: every library the files in ROOT need from this Qt or the stage, plus jemalloc and libgomp
# (Kirigami's OpenMP; neither is on every distro by default), into ROOT/lib; UNDER is the bundle ROOT unpacks over,
# whose libraries stay there
carry() {
    local root=$1 under=${2:-} obj lib base objects
    local -A have=()
    mapfile -t objects < <(elves "$root")
    while ((${#objects[@]})); do
        obj=${objects[0]}
        objects=("${objects[@]:1}")
        while read -r lib; do
            case "$lib" in "$QT"/lib/* | "$STAGE"/lib/* | */libjemalloc.so* | */libgomp.so*) ;; *) continue ;; esac
            base=${lib##*/}
            [ -n "${have[$base]:-}" ] && continue
            have[$base]=1
            cp -L "$lib" "$root/lib/$base"
            objects+=("$root/lib/$base")
        done < <(LD_LIBRARY_PATH="$root/lib${under:+:$under/lib}:$QT/lib:$STAGE/lib" needs "$obj")
    done
    # each file finds lib/ from where it sits
    while read -r obj; do
        patchelf --set-rpath "\$ORIGIN/$(realpath --relative-to="$(dirname "$obj")" "$root/lib")" "$obj"
    done < <(elves "$root" ! -path "$root/lib/*")
    find "$root/lib" -type f -name '*.so*' -exec patchelf --set-rpath '$ORIGIN' {} \;
    # nothing left out: every library resolves from the bundle or the system (the build host's system libs are the
    # ones every desktop has)
    lib=$(elves "$root" -exec env LD_LIBRARY_PATH="${under:+$under/lib}" ldd {} + 2>/dev/null | grep 'not found' | sort -u || true)
    [ -z "$lib" ] || { printf 'unresolved:\n%s\n' "$lib" >&2; exit 1; }
}
carry "$DEST"
[ -f "$DEST/plugins/platforms/libqwayland.so" ] || { echo "no wayland platform plugin" >&2; exit 1; }

# Qt, KDE parts, jemalloc and libgomp ride unmodified and dynamically linked: their licenses and where their sources are
QTVER=$("$QT/bin/qmake" -query QT_VERSION)
mkdir -p "$DEST/share/licenses"
cp "$ROOT"/LICENSE* "$DEST/share/licenses/"
mkdir -p "$DEST/share/licenses/qt-sbom"
find "$QT/sbom" -name '*.spdx' ! -name '*.source.spdx' ! -name 'qtwebengine*' ! -name 'qtwebchannel*' \
    ! -name 'qtpositioning*' -exec cp {} "$DEST/share/licenses/qt-sbom/" \;
for part in layer-shell-qt kirigami syntax-highlighting; do cp -a "$WORK/$part/LICENSES" "$DEST/share/licenses/$part"; done
# as the build's distro keeps them: Ubuntu in CI, Arch on a dev machine
lic() { local f; for f in "${@:2}"; do [ -e "$f" ] && { cp -a "$f" "$DEST/share/licenses/$1"; return; }; done; echo "no license for $1" >&2; return 1; }
lic jemalloc /usr/share/doc/libjemalloc2/copyright /usr/share/licenses/jemalloc/COPYING
lic libgomp /usr/share/doc/libgomp1/copyright /usr/share/licenses/libgomp
cat >"$DEST/SOURCES" <<EOF
qs-rs $VER for x86_64 with its own Qt: unpack it anywhere and run bin/qs. glibc, mesa, wayland, pam, pipewire,
dbus and fonts come from the system.

What lib/, plugins/ and qml/ carry is unmodified and linked dynamically; licenses in share/licenses. Sources:
- qs-rs: this version's tag at https://github.com/snowarch/qs-rs
- Qt $QTVER, with the FFmpeg and ICU it ships (share/licenses/qt-sbom names each):
  https://download.qt.io/archive/qt/${QTVER%.*}/$QTVER/single/qt-everywhere-src-$QTVER.tar.xz
- layer-shell-qt $PLASMA: https://invent.kde.org/plasma/layer-shell-qt/-/tree/$PLASMA
- Kirigami and KSyntaxHighlighting $KF: https://invent.kde.org/frameworks/kirigami/-/tree/$KF,
  https://invent.kde.org/frameworks/syntax-highlighting/-/tree/$KF
- jemalloc and libgomp: the jemalloc and gcc source packages of Ubuntu 24.04
EOF

# same bytes for the same commit: names sorted, no build user, every time the commit's
EPOCH=${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct 2>/dev/null || date +%s)}
pack() {
    # gzip: every distro unpacks it with nothing more installed
    tar -C "$OUT" --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$EPOCH" -cf - "$1" | gzip -9 -n >"$OUT/$1.tar.gz"
    (cd "$OUT" && sha256sum "$1.tar.gz" >"$1.tar.gz.sha256")
    echo "$OUT/$1.tar.gz ($(du -h "$OUT/$1.tar.gz" | cut -f1))"
}
pack "$NAME"

# Qt WebEngine, for the sidebar's web apps: 3x the rest of Qt for one tab most people leave off, so it is its own
# download, unpacked over the bundle (Qt finds libexec/, resources/ and translations/ from bin/qt.conf's prefix)
[ -f "$QT/lib/libQt6WebEngineCore.so.6" ] || { echo "no Qt WebEngine in $QT: qs-rs-webengine not built"; exit 0; }
WEB=$OUT/qs-rs-webengine-$VER-x86_64
rm -rf "$WEB"
mkdir -p "$WEB/lib" "$WEB/libexec" "$WEB/qml" "$WEB/translations" "$WEB/share/licenses/qt-sbom"
cp -a "$QT/libexec/QtWebEngineProcess" "$WEB/libexec/"
printf '[Paths]\nPrefix = ..\nLibraries = lib\nPlugins = plugins\nQmlImports = qml\n' >"$WEB/libexec/qt.conf"
cp -a "$QT/resources" "$WEB/"
cp -a "$QT/translations/qtwebengine_locales" "$WEB/translations/"
for mod in QtWebEngine QtWebChannel; do
    mkdir -p "$WEB/qml/$mod" && cp -a "$QT/qml/$mod/." "$WEB/qml/$mod/"
done
find "$WEB/qml" -type d -name designer -prune -exec rm -rf {} +
carry "$WEB" "$DEST"
find "$QT/sbom" \( -name 'qtwebengine*' -o -name 'qtwebchannel*' -o -name 'qtpositioning*' \) -name '*.spdx' \
    ! -name '*.source.spdx' -exec cp {} "$WEB/share/licenses/qt-sbom/" \;
pack "${WEB##*/}"
