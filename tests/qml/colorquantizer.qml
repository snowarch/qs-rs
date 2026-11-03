import QtQuick
import Quickshell

// ColorQuantizer: median-cut colours of a picture (left half red, right half blue), cropped and shrunk by the
// decoder, and two quantizers on the same picture at once (they share one decode).
ShellRoot {
    id: root
    property var errors: []
    readonly property url halves: Qt.resolvedUrl("quantizer/halves.jpg")
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function near(c, r, g, b) { return Math.abs(c.r * 255 - r) < 24 && Math.abs(c.g * 255 - g) < 24 && Math.abs(c.b * 255 - b) < 24 }
    function finish() {
        const two = Array.from(both.colors)
        check("two colours from a shrunk read", two.length === 2)
        check("they are the red and the blue", two.some(c => near(c, 230, 20, 20)) && two.some(c => near(c, 20, 40, 230)))
        check("a crop reads only its part", left.colors.length === 1 && near(left.colors[0], 230, 20, 20))
        check("a crop past the edge keeps what is inside", right.colors.length === 1 && near(right.colors[0], 20, 40, 230))
        check("the same picture asked twice at once", twin.colors.length === 1 && near(twin.colors[0], same.colors[0].r * 255, same.colors[0].g * 255, same.colors[0].b * 255))
        check("a whole read with no shrink", whole.colors.length === 1 && near(whole.colors[0], 125, 30, 125))
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }

    ColorQuantizer { id: both; source: root.halves; depth: 1; rescaleSize: 16 }
    ColorQuantizer { id: left; source: root.halves; depth: 0; rescaleSize: 8; imageRect: Qt.rect(0, 0, 800, 800) }
    ColorQuantizer { id: right; source: root.halves; depth: 0; rescaleSize: 8; imageRect: Qt.rect(900, 100, 2000, 2000) }
    ColorQuantizer { id: same; source: root.halves; depth: 0; rescaleSize: 10 }
    ColorQuantizer { id: twin; source: root.halves; depth: 0; rescaleSize: 10 }
    ColorQuantizer { id: whole; source: root.halves; depth: 0 }

    Timer {
        interval: 50; repeat: true; running: true
        property int waited: 0
        onTriggered: {
            waited += 50
            const all = [both, left, right, same, twin, whole]
            if (all.every(q => q.colors.length > 0) || waited > 8000) { running = false; root.finish() }
        }
    }
}
