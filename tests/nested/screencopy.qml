import QtQuick
import Quickshell
import Quickshell.Wayland

// Needs a compositor with wlr-screencopy (niri). Never run against the real session:
//   scripts/nested-niri.sh /tmp/x.png 3 qs-rs -p tests/nested/screencopy.qml   (prints PASS/FAIL)
// A full-screen panel paints four quadrants (red, green, blue, white); a ScreencopyView of the
// screen sits in the white one. Checks: the first frame arrives with the screen's pixel size,
// constraintSize keeps the aspect ratio, a one-shot captureFrame on a still screen returns
// (copy_with_damage would wait forever), live keeps frames coming while something moves and
// stops when turned off, the captured pixels are the quadrant colors where they belong, and a
// null source drops the content.
ShellRoot {
    id: root
    property var log: []
    property int step: 0
    property int frames: 0
    function check(name, ok) { if (!ok) log.push("failed: " + name) }
    function finish() {
        console.log(log.length ? "FAIL " + log.join(", ") : "PASS")
        Qt.exit(log.length ? 1 : 0)
    }

    PanelWindow {
        id: panel
        screen: Quickshell.screens[0]
        anchors { top: true; bottom: true; left: true; right: true }
        exclusionMode: ExclusionMode.Ignore
        color: "white"
        Rectangle { x: 0; y: 0; width: parent.width / 2; height: parent.height / 2; color: "#ff0000" }
        Rectangle { x: parent.width / 2; y: 0; width: parent.width / 2; height: parent.height / 2; color: "#00ff00" }
        Rectangle { x: 0; y: parent.height / 2; width: parent.width / 2; height: parent.height / 2; color: "#0000ff" }
        // Moves only while the live check runs, so the screen is still otherwise.
        Rectangle {
            id: mover
            width: 20; height: 20; y: 10; color: "black"
            NumberAnimation on x { id: moving; running: false; from: 10; to: 300; duration: 1000; loops: Animation.Infinite }
        }
        ScreencopyView {
            id: view
            x: parent.width * 3 / 4 - width / 2
            y: parent.height * 3 / 4 - height / 2
            width: implicitWidth; height: implicitHeight
            captureSource: Quickshell.screens[0]
            constraintSize: Qt.size(160, 160)
        }
        // Reads the view's pixels back: the grab goes through an Image, which a Canvas can draw
        // (both only work inside a window).
        Image {
            id: grabbed
            visible: false
            onStatusChanged: if (status === Image.Ready) root.readPixels()
        }
        Canvas {
            id: canvas
            x: parent.width - width - 10; y: parent.height / 2 + 10
            width: view.width; height: view.height
        }
    }

    Connections {
        target: view.data[0]
        function onFrameChanged() { if (view.data[0].frame !== undefined) root.frames++ }
    }


    function readPixels() {
        const ctx = canvas.getContext("2d")
        const w = view.width, h = view.height
        ctx.drawImage(grabbed, 0, 0, w, h)
        const at = (fx, fy) => { const d = ctx.getImageData(Math.floor(w * fx), Math.floor(h * fy), 1, 1).data; return [d[0], d[1], d[2]] }
        const near = (p, q) => Math.abs(p[0] - q[0]) < 40 && Math.abs(p[1] - q[1]) < 40 && Math.abs(p[2] - q[2]) < 40
        const expect = (name, fx, fy, rgb) => { const p = at(fx, fy); check(name + " (" + p + ")", near(p, rgb)) }
        expect("top left is red", 0.25, 0.25, [255, 0, 0])
        expect("top right is green", 0.75, 0.25, [0, 255, 0])
        expect("bottom left is blue", 0.25, 0.75, [0, 0, 255])
        expect("bottom right is white", 0.9, 0.9, [255, 255, 255])
        root.step = 4
        tick.restart()
    }

    Timer {
        id: tick
        interval: 1000
        running: true
        onTriggered: {
            const s = Quickshell.screens[0]
            if (root.step === 0) {
                check("first frame", view.hasContent)
                const pw = Math.round(s.width * s.devicePixelRatio), ph = Math.round(s.height * s.devicePixelRatio)
                check("source size " + view.sourceSize.width + "x" + view.sourceSize.height + " vs " + pw + "x" + ph,
                      view.sourceSize.width === pw && view.sourceSize.height === ph)
                const k = Math.min(160 / pw, 160 / ph)
                check("constrained size", Math.abs(view.implicitWidth - pw * k) < 0.5 && Math.abs(view.implicitHeight - ph * k) < 0.5)
                root.frames = 0
                view.captureFrame()
                root.step = 1
            } else if (root.step === 1) {
                check("one-shot frame on a still screen (" + root.frames + ")", root.frames === 1)
                root.frames = 0
                moving.running = true
                view.live = true
                root.step = 2
            } else if (root.step === 2) {
                check("live frames while moving (" + root.frames + ")", root.frames >= 10)
                view.live = false
                moving.running = false
                root.frames = 0
                root.step = 3
            } else if (root.step === 3) {
                check("live off stops frames (" + root.frames + ")", root.frames <= 1)
                view.grabToImage(r => { grabbed.source = r.url })
            } else if (root.step === 4) {
                view.captureSource = null
                // Re-assigning a screen recaptures without a "non captureable" warning.
                view.captureSource = Quickshell.screens[0]
                view.captureSource = null
                root.step = 5
            } else {
                check("null source drops content", !view.hasContent && view.sourceSize.width === 0)
                root.finish()
            }
        }
        repeat: root.step !== 3
    }
}
