import QtQuick
import Quickshell

// Driven by tests/peaks/wrap.sh: after the startup line, a window shows and takes ~150 MB;
// the memory peak log has to name it.
ShellRoot {
    id: root
    property var ballast: []
    Timer {
        interval: 7000; running: true
        onTriggered: { heavy.visible = true; console.log("SHOWN") }
    }
    FloatingWindow {
        id: heavy
        visible: false
        implicitWidth: 100; implicitHeight: 100
        onVisibleChanged: if (visible) {
            const a = []
            for (let i = 0; i < 2000000; i++) a.push({ i: i, s: "x" + i })
            root.ballast = a
        }
    }
}
