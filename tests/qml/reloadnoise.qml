import QtQuick
import Quickshell
import Quickshell.Io

// A log line that merely reads like a failed load keeps no window from the next generation: an info line (inir's
// Weather says "GPS unavailable" at info, and QtInfoMsg sorts above QtWarningMsg) or a script's own warning.
ShellRoot {
    id: root
    FileView { id: marker; path: Quickshell.statePath("reloadnoise-generation"); blockLoading: true; printErrors: false }
    readonly property int generation: parseInt(marker.text() || "0")

    FloatingWindow { id: shown; implicitWidth: 120; implicitHeight: 80 }

    Timer {
        running: true
        interval: 300
        onTriggered: {
            if (root.generation === 0) {
                console.info("[Weather] GPS unavailable, using IP location")
                console.warn("a source is unavailable for now")
                shown._backingWindow.objectName = "kept"
                marker.setText("1")
                Quickshell.reload(false)
                return
            }
            const ok = shown._backingWindow.objectName === "kept"
            if (!ok) console.log("check failed: a log line that only reads like a failure rebuilt the window")
            Qt.exit(ok ? 0 : 1)
        }
    }
    Timer { interval: 5000; running: root.generation === 0; onTriggered: { console.log("FAILED: no reload"); Qt.exit(1) } }
}
