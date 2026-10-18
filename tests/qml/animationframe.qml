import QtQuick
import Quickshell

// Each frame shows animations at its own time, not at the last driver tick before it (a
// free-running tick beats against the real vblank: doubled or missing steps). Here the window
// draws continuously at the platform's pace while the driver's fallback ticks at 23 Hz
// (wrap.sh), like a compositor whose refresh is not the driver's; a 1 px/ms slide sampled at
// every frame must lag the clock by a constant.
ShellRoot {
    id: root
    property real value: 0
    property real start: Date.now()
    property real minLag: 1e9
    property real maxLag: -1e9
    property int frames: 0

    NumberAnimation on value { from: 0; to: 1000000; duration: 1000000; running: true }

    FloatingWindow {
        id: win
        implicitWidth: 64
        implicitHeight: 64
        Rectangle { width: 10; height: 10; color: "red"; x: root.value % 50 }
    }
    Connections {
        target: win._backingWindow
        function onFrameSwapped() { win._backingWindow.update() }
        function onBeforeSynchronizing() {
            if (++root.frames < 20) return
            const lag = (Date.now() - root.start) - root.value
            root.minLag = Math.min(root.minLag, lag)
            root.maxLag = Math.max(root.maxLag, lag)
        }
    }
    Timer {
        interval: 1500
        running: true
        onTriggered: {
            const spread = root.maxLag - root.minLag
            const ok = root.frames > 60 && spread < 5
            if (!ok) console.log("check failed: frames " + root.frames + ", lag spread " + spread + " ms")
            Qt.exit(ok ? 0 : 1)
        }
    }
}
