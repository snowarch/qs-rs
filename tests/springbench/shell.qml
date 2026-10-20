// The bench shell: see run.sh.
import QtQuick
import Quickshell
ShellRoot {
    id: root
    readonly property string impl: Quickshell.env("IMPL") || "native"
    readonly property int count: Number(Quickshell.env("N") || 1)
    readonly property real load: Number(Quickshell.env("LOAD") || 0)
    property real target: 0
    property var frames: []
    property bool recording: false
    Component { id: jsSpring; JsSpring { to: root.target; springParams: ({ response: 300, bounce: 0 }) } }
    Component { id: nativeSpring; SpringValue { to: root.target; emerge: ({ response: 300, bounce: 0 }) } }
    property var springs: []
    Component.onCompleted: {
        const c = impl === "js" ? jsSpring : nativeSpring
        const list = []
        for (let i = 0; i < count; i++) list.push(c.createObject(root))
        springs = list
    }
    FloatingWindow {
        id: win
        implicitWidth: 500; implicitHeight: 60; color: "black"
        Rectangle { width: 20; height: 20; color: "red"; x: root.springs.length ? root.springs[0].value : 0 }
    }
    property real pending: 0
    Connections {
        target: win._backingWindow
        function onBeforeSynchronizing() { root.pending = root.springs.length ? root.springs[0].value : 0 }
        function onFrameSwapped() {
            if (root.recording) root.frames.push([Date.now(), root.pending, root.target])
            if (root.load > 0) { const t = Date.now(), w = Math.random() * root.load; while (Date.now() - t < w) {} }
        }
    }
    Timer { interval: 900; running: true; repeat: true; onTriggered: { root.recording = true; root.target = root.target > 0 ? 0 : 400 } }
    Timer { interval: 900 * 8 + 450; running: true; onTriggered: { console.log("FRAMES " + JSON.stringify(root.frames)); Qt.quit() } }
}
