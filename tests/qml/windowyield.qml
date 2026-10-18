import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland

// A reload into another layout (inir switching families): the old generation's frozen panels
// nobody claims give way as soon as a new panel shows in their layer, instead of covering it
// until the pool drops them. A panel in another layer keeps waiting for its claim.
ShellRoot {
    id: root
    FileView { id: marker; path: Quickshell.statePath("windowyield-generation"); blockLoading: true; printErrors: false }
    readonly property int generation: parseInt(marker.text() || "0")

    // Only the old generation declares these, like a family the new config no longer loads.
    LazyLoader {
        active: root.generation === 0
        component: Scope {
            PanelWindow {
                WlrLayershell.namespace: "old-family-bar"
                anchors { left: true; right: true; top: true }
                implicitHeight: 30
            }
            PanelWindow {
                WlrLayershell.layer: WlrLayer.Bottom
                WlrLayershell.namespace: "old-family-zone"
                anchors { left: true; top: true; bottom: true }
                implicitWidth: 10
            }
        }
    }
    PanelWindow {
        id: fresh
        visible: root.generation === 1
        WlrLayershell.namespace: "new-family-bar"
        anchors { left: true; right: true; top: true }
        implicitHeight: 40
    }

    function check(name, ok) { if (!ok) console.log("check failed: " + name); return ok }

    Timer {
        running: true
        interval: 300
        onTriggered: {
            if (root.generation === 0) {
                marker.setText("1")
                Quickshell.reload(false)
                return
            }
            let ok = check("new panel shown", fresh.backingWindowVisible)
            ok = check("only the other layer's panel still frozen: " + QsWindowSupport.stashedWindows(), QsWindowSupport.stashedWindows() === 1) && ok
            Qt.exit(ok ? 0 : 1)
        }
    }
    Timer { interval: 5000; running: root.generation === 0; onTriggered: { console.log("FAILED: no reload"); Qt.exit(1) } }
}
