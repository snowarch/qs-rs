import QtQuick
import Quickshell
import Quickshell.Io

// A part that fails while the first generation is created counts like one in any later generation: its windows are
// rebuilt by the next reload, never reused half built.
ShellRoot {
    id: root
    FileView { id: marker; path: Quickshell.statePath("reloadfirstfail-generation"); blockLoading: true; printErrors: false }
    readonly property int generation: parseInt(marker.text() || "0")

    FloatingWindow { id: shown; implicitWidth: 120; implicitHeight: 80 }
    Loader { active: root.generation === 0; source: "brokenlib/Broken.qml" }

    function check(name, ok) { if (!ok) console.log("check failed: " + name); return ok }

    Timer {
        running: true
        interval: 300
        onTriggered: {
            if (root.generation === 0) {
                shown._backingWindow.objectName = "half built"
                marker.setText("1")
                Quickshell.reload(false)
                return
            }
            let ok = root.check("the failed generation's window was rebuilt", shown._backingWindow.objectName !== "half built")
            ok = root.check("the rebuilt window is shown", shown.backingWindowVisible) && ok
            Qt.exit(ok ? 0 : 1)
        }
    }
    Timer { interval: 5000; running: root.generation === 0; onTriggered: { console.log("FAILED: no reload"); Qt.exit(1) } }
}
