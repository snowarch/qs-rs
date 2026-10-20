import QtQuick
import Quickshell
import Quickshell.Io
import "teardownlib"

// A reload tears down the old engine with its singletons. Bindings in one singleton that read
// another (through an object property whose target dies first) must not run against the dead
// one: upstream interrupts JS before the teardown. Several reloads in a row, as watchFiles does.
ShellRoot {
    id: root
    FileView { id: marker; path: Quickshell.statePath("teardown-generation"); blockLoading: true; printErrors: false }
    readonly property int generation: parseInt(marker.text() || "0")
    readonly property int sum: Holder1.seen + Holder2.seen + Holder3.seen + Holder4.seen + Holder5.seen + Holder6.seen

    Component.onCompleted: {
        if (generation < 4) {
            marker.setText(String(generation + 1))
            Quickshell.reload(false)
        } else {
            Qt.exit(root.sum === 6 ? 0 : 1)
        }
    }
    Timer { interval: 5000; running: true; onTriggered: { console.log("FAILED: stuck at " + root.generation); Qt.exit(1) } }
}
