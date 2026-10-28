import QtQuick
import Quickshell
import Quickshell.Io

// Driven by tests/peaks/wrap.sh: after the startup line, a window shows and takes ~150 MB;
// the memory peak log has to name it. It waits for that line itself: a loaded machine writes it late.
ShellRoot {
    id: root
    property var ballast: []
    FileView { id: peaks; path: Quickshell.env("XDG_STATE_HOME") + "/qs-rs/memory-peaks" }
    Timer {
        interval: 200; repeat: true; running: !heavy.visible
        onTriggered: {
            peaks.reload()
            if (peaks.text().includes("startup peak")) { heavy.visible = true; console.log("SHOWN") }
        }
    }
    FloatingWindow {
        id: heavy
        visible: false
        implicitWidth: 100; implicitHeight: 100
        onVisibleChanged: if (visible) {
            // filled, so every page is resident at once (2M JS objects took seconds on a busy machine)
            root.ballast = new Uint8Array(150 * 1024 * 1024).fill(1)
        }
    }
}
