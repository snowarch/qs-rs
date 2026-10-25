import QtQuick
import Quickshell

// PopupWindow API and anchor math: defaults, enums, the anchor rect of an item mapped into its
// window (minus margins), deprecated relativeX/Y, and showing only with a visible parent.
// Placement on a compositor is checked visually under scripts/nested.sh.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }

    FloatingWindow {
        id: parentWin
        implicitWidth: 300; implicitHeight: 200
        Item { id: holder; x: 20; y: 30; Rectangle { id: btn; x: 10; y: 5; width: 40; height: 20 } }
    }
    PopupWindow {
        id: itemPopup
        anchor.item: btn
        anchor.margins.left: 2
        anchor.margins.bottom: 4
        anchor.edges: Edges.Bottom | Edges.Left
        implicitWidth: 50; implicitHeight: 50
    }
    PopupWindow {
        id: winPopup
        parentWindow: parentWin
        relativeX: 7
        relativeY: 9
        visible: true
        property int closes: 0
        onClosed: closes++
        implicitWidth: 50; implicitHeight: 50
    }
    PopupWindow { id: orphan; visible: true }

    Timer {
        interval: 600
        running: true
        onTriggered: {
            root.check("enums", Edges.Top === 1 && Edges.Left === 2 && Edges.Right === 4 && Edges.Bottom === 8
                && PopupAdjustment.Slide === 3 && PopupAdjustment.All === 63)
            root.check("defaults", !itemPopup.visible && !itemPopup.grabFocus && winPopup.anchor.gravity === (Edges.Bottom | Edges.Right)
                && winPopup.anchor.edges === (Edges.Top | Edges.Left) && winPopup.anchor.adjustment === PopupAdjustment.Slide)
            // Upstream: with an item, `window` reads the Quickshell window the item is in.
            root.check("item window", itemPopup.anchor.window === parentWin && itemPopup.anchor.item === btn)
            itemPopup.anchor.updateAnchor()
            const r = itemPopup.anchor._windowRect
            root.check("item rect " + r, r.x === 32 && r.y === 35 && r.width === 38 && r.height === 16)
            // A rect set on an item anchor is the item's, mapped at once (a resize while shown looped the placement).
            itemPopup.anchor.rect.width = 30
            itemPopup.anchor.rect.height = 10
            const m = itemPopup.anchor._windowRect
            root.check("item rect set " + m, m.x === 32 && m.y === 35 && m.width === 28 && m.height === 6)
            itemPopup.anchor.rect.width = 0
            itemPopup.anchor.rect.height = 0
            // Item gone: no window either; setting a window drops the item (upstream setWindow).
            itemPopup.anchor.item = null
            root.check("no item, no window", itemPopup.anchor.window === null)
            itemPopup.anchor.item = btn
            itemPopup.anchor.window = parentWin
            root.check("window drops item", itemPopup.anchor.item === null && itemPopup.anchor.window === parentWin)
            itemPopup.anchor.item = btn
            root.check("relative", winPopup.anchor.window === parentWin && winPopup.anchor.rect.x === 7 && winPopup.anchor._windowRect.y === 9)
            root.check("hidden popup stays hidden", !itemPopup.backingWindowVisible)
            root.check("shown with parent", parentWin._mapped && winPopup.backingWindowVisible)
            root.check("orphan not shown", !orphan.backingWindowVisible)
            winPopup.visible = false
            winPopup.visible = true
            root.check("own hide is not a close", winPopup.closes === 0 && winPopup.backingWindowVisible)
            winPopup._window.close()
            root.check("external close", winPopup.closes === 1 && !winPopup.visible && !winPopup.backingWindowVisible)
            root.finish()
        }
    }
}
