import QtQuick
import Quickshell

// Animations step once per frame, and the frame meter (`qs frames`) sees what a window really
// draws. A 60 Hz headless KWin (wrap.sh) shows a slide for 2 s light, then 2 s with 25 ms of JS
// in every step: the step no longer fits a refresh, so each frame takes two. wrap.sh judges the
// counts printed here and the meter's lines.
ShellRoot {
    id: root
    property real value: 0
    property int steps: 0
    property int frames: 0
    property real load: 0
    property int phase: 0

    onValueChanged: {
        root.steps++
        if (root.load > 0) {
            const t = Date.now()
            while (Date.now() - t < root.load) {}
        }
    }
    NumberAnimation on value { from: 0; to: 1000000; duration: 1000000; running: true }

    FloatingWindow {
        id: win
        implicitWidth: 200
        implicitHeight: 100
        color: "black"
        Rectangle { width: 20; height: 20; color: "red"; x: root.value % 180 }
    }
    Connections {
        target: win._backingWindow
        function onFrameSwapped() { root.frames++ }
    }
    // 1 s to settle, then each phase starts on a clean count.
    Timer {
        interval: 1000
        running: true
        repeat: true
        onTriggered: {
            if (root.phase === 1 || root.phase === 3) console.log("phase " + (root.phase === 1 ? "light" : "load") + " steps " + root.steps + " frames " + root.frames)
            if (root.phase === 3) { console.log("done"); interval = 100000 }
            if (root.phase === 1) root.load = 25
            root.phase++
            root.steps = 0
            root.frames = 0
            if (root.phase === 1 || root.phase === 3) interval = 2000
            else interval = 200
        }
    }
}
