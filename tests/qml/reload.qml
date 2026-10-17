import QtQuick
import Quickshell
import Quickshell.Io
import "reloadlib"

// Hot reload as upstream: Quickshell.reload() builds a new generation (fresh singletons), the
// old one goes away, and the new one gets reloadCompleted. The generation count lives in a file,
// since a reload starts this file over.
ShellRoot {
    id: root
    FileView { id: marker; path: Quickshell.statePath("reload-test-generation"); blockLoading: true; printErrors: false }
    readonly property int generation: parseInt(marker.text() || "0")
    property bool completed: false

    Connections {
        target: Quickshell
        function onReloadCompleted() { root.completed = true }
    }

    Component.onCompleted: {
        if (generation === 0) {
            Generation.touched = 42
            marker.setText("1")
            Quickshell.reload(false)
        } else {
            done.start()
        }
    }
    Timer {
        id: done
        interval: 300
        onTriggered: {
            const ok = root.completed && Generation.touched === 0
            if (!ok) console.log("FAILED: completed=" + root.completed + " touched=" + Generation.touched)
            Qt.exit(ok ? 0 : 1)
        }
    }
    // Generation 0 must not outlive the reload.
    Timer { interval: 5000; running: root.generation === 0; onTriggered: { console.log("FAILED: no reload"); Qt.exit(1) } }
}
