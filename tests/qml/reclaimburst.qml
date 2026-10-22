import QtQuick
import Quickshell

// Driven by tests/reclaimburst/wrap.sh: a window that stays shown drops a tree of ~6000 objects
// (what a Design switch or a closed page does); memory has to be reclaimed without any window
// hiding. A small drop (300 objects) earlier must not start a reclaim.
ShellRoot {
    FloatingWindow {
        implicitWidth: 200; implicitHeight: 200
        visible: true
        Loader {
            id: small
            active: true
            sourceComponent: Item { Repeater { model: 300; Item {} } }
        }
        Loader {
            id: big
            active: true
            sourceComponent: Item { Repeater { model: 6000; Item {} } }
        }
    }
    Timer { interval: 1000; running: true; onTriggered: { small.active = false; console.log("SMALL DROPPED") } }
    Timer { interval: 5000; running: true; onTriggered: { big.active = false; console.log("BIG DROPPED") } }
    Timer { interval: 10000; running: true; onTriggered: Qt.exit(0) }
}
