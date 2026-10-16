import QtQuick
import Quickshell

// Input masks follow upstream: the default intersection keeps the region as is, so an empty
// Region makes a window click-through and an item region accepts input only there.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) errors.push(name) }
    function passThrough(w) { return (w._backingWindow.flags & Qt.WindowTransparentForInput) !== 0 }

    PanelWindow { id: empty; implicitWidth: 200; implicitHeight: 50; mask: Region {} }
    PanelWindow {
        id: itemMask; implicitWidth: 200; implicitHeight: 50
        mask: Region { item: box }
        Rectangle { id: box; x: 10; width: 30; height: 20 }
    }
    PanelWindow { id: inverted; implicitWidth: 200; implicitHeight: 50; mask: Region { intersection: Intersection.Xor } }
    PanelWindow { id: none; implicitWidth: 200; implicitHeight: 50 }

    Timer {
        interval: 100; running: true
        onTriggered: {
            root.check("empty.passThrough", root.passThrough(empty))
            root.check("item.accepts", !root.passThrough(itemMask))
            root.check("item.region", QsWindowSupport.maskRect(itemMask._backingWindow) === "10,0 30x20")
            root.check("xor.full", !root.passThrough(inverted) && QsWindowSupport.maskRect(inverted._backingWindow) === "0,0 200x50")
            root.check("none.accepts", !root.passThrough(none))
            if (root.errors.length) console.log("FAILED: " + root.errors.join(", "))
            Qt.exit(root.errors.length ? 1 : 0)
        }
    }
}
