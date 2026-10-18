import QtQuick
import Quickshell
import Quickshell.Io

// Window reuse across a reload (upstream ProxyWindowBase::onReload): the new generation's
// proxy takes back the shown window of the same declaration, with its own content inside;
// hidden windows are not kept. The generation lives in a file, since a reload starts over.
ShellRoot {
    id: root
    FileView { id: marker; path: Quickshell.statePath("windowreuse-generation"); blockLoading: true; printErrors: false }
    readonly property int generation: parseInt(marker.text() || "0")

    FloatingWindow {
        id: shown
        implicitWidth: 120
        implicitHeight: 80
        Rectangle { id: inside; anchors.fill: parent; color: "red" }
    }
    FloatingWindow { id: hidden; visible: false }

    function check(name, ok) { if (!ok) console.log("check failed: " + name); return ok }

    Timer {
        running: true
        interval: 300
        onTriggered: {
            if (root.generation === 0) {
                shown._backingWindow.objectName = "kept"
                hidden._backingWindow.objectName = "dropped"
                marker.setText("1")
                Quickshell.reload(false)
                return
            }
            let ok = check("shown window reused", shown._backingWindow.objectName === "kept")
            ok = check("hidden window not reused", hidden._backingWindow.objectName !== "dropped") && ok
            ok = check("reused window still shown", shown.backingWindowVisible) && ok
            ok = check("new content is in the reused window", inside.Window.window === shown._backingWindow) && ok
            ok = check("content sized to the window", inside.width === 120 && inside.height === 80) && ok
            Qt.exit(ok ? 0 : 1)
        }
    }
    Timer { interval: 5000; running: root.generation === 0; onTriggered: { console.log("FAILED: no reload"); Qt.exit(1) } }
}
