import QtQuick
import Quickshell

// updatesEnabled as upstream: off, the render loop skips the window (no frame while its content
// keeps changing, the compositor keeps the last one); on again, it draws.
ShellRoot {
    FloatingWindow {
        id: win
        implicitWidth: 100; implicitHeight: 100
        property int frames: 0
        Rectangle { width: 50; height: 50; NumberAnimation on rotation { from: 0; to: 360; duration: 500; loops: Animation.Infinite } }
        property Connections _count: Connections {
            target: win._backingWindow
            function onFrameSwapped() { win.frames++ }
        }
    }
    property var steps: [
        () => { if (win.frames === 0) return "no frames while on"; win.updatesEnabled = false },
        () => { win.frames = 0 },
        () => { if (win.frames !== 0) return win.frames + " frames while off"; win.updatesEnabled = true },
        () => { if (win.frames === 0) return "no frames after turning it on" },
    ]
    property int step: 0
    Timer {
        interval: 300; repeat: true; running: true
        onTriggered: {
            const err = steps[step]()
            if (err) { console.log("FAILED", err); Qt.exit(1) }
            if (++step === steps.length) Qt.exit(0)
        }
    }
}
