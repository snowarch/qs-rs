import QtQuick
import Quickshell
import Quickshell.Io

// A panel stretched between opposite edges is sized by the compositor, never by its implicit
// size: a reload that hands its window to the new generation must not shrink it to 100 (inir's
// desktop widgets got clamped into that and saved it as a layout).
ShellRoot {
    id: root
    FileView { id: marker; path: Quickshell.statePath("panelreuse-generation"); blockLoading: true; printErrors: false }
    readonly property int generation: parseInt(marker.text() || "0")
    property int narrowest: 100000

    PanelWindow {
        id: panel
        anchors { left: true; right: true; top: true }
        implicitHeight: 40
        Item {
            id: inside
            anchors.fill: parent
            onWidthChanged: if (root.generation > 0 && panel._backingWindow && width < root.narrowest) root.narrowest = width
        }
    }

    function check(name, ok) { if (!ok) console.log("check failed: " + name); return ok }

    Timer {
        running: true
        interval: 300
        onTriggered: {
            if (root.generation === 0) {
                // What the compositor's configure does.
                panel._backingWindow.width = 777
                panel._backingWindow.objectName = "kept"
                marker.setText("1")
                Quickshell.reload(false)
                return
            }
            let ok = check("panel window reused", panel._backingWindow.objectName === "kept")
            ok = check("width kept: " + panel._backingWindow.width, panel._backingWindow.width === 777) && ok
            ok = check("content never saw the implicit size: " + root.narrowest, root.narrowest >= 777) && ok
            ok = check("content sized to the window", inside.width === 777) && ok
            Qt.exit(ok ? 0 : 1)
        }
    }
    Timer { interval: 5000; running: root.generation === 0; onTriggered: { console.log("FAILED: no reload"); Qt.exit(1) } }
}
