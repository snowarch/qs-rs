import QtQuick
import Quickshell
import Quickshell.Io

// Driven by tests/fontrelease/wrap.sh in a headless KWin, so text goes through the GPU glyph
// caches (distance field and native). A shown window drops the only texts drawn in two fonts (what leaving a Design does):
// once the reclaim after that teardown has run, the font file is no longer mapped, while the
// font of a text that stays is.
ShellRoot {
    FloatingWindow {
        implicitWidth: 400; implicitHeight: 300
        visible: true
        Text { y: 280; text: "kept"; font.family: "DejaVu Sans" }
        Loader {
            id: design
            sourceComponent: Column {
                Repeater { model: 2000; Text { text: "AIRISU " + index; font.family: "Noto Serif" } }
                Repeater { model: 500; Text { text: "native " + index; font.family: "DejaVu Serif"; renderType: Text.NativeRendering } }
            }
        }
    }
    Process {
        id: maps
        property string tag
        command: ["sh", "-c", "grep -o 'NotoSerif-Regular.ttf\\|DejaVuSerif.ttf\\|DejaVuSans.ttf' /proc/$PPID/maps | sort -u | tr '\\n' ' '"]
        stdout: StdioCollector { onStreamFinished: console.log("FONTS " + maps.tag + ": " + text) }
    }
    function check(tag) { maps.tag = tag; maps.running = true }
    Timer { interval: 2000; running: true; onTriggered: { check("shown"); design.active = false } }
    Timer { interval: 10000; running: true; onTriggered: check("dropped") }
    Timer { interval: 11000; running: true; onTriggered: Qt.exit(0) }
}
