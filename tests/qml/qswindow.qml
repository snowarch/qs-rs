import QtQuick
import Quickshell

// QsWindow.window from content, as bindings read it while the window is being built: the item
// enters its QQuickWindow before the window registers its Quickshell object, so a binding that
// only re-runs on the item's window change stays null (inir's bar read `screen` that way and
// its scroll-for-brightness failed). Also upstream's screen default: a window that never sets
// `screen` reads the screen it is on, not null.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }

    FloatingWindow {
        id: withScreen
        screen: Quickshell.screens[0]
        implicitWidth: 100; implicitHeight: 100
        Item {
            id: probeA
            property var win: QsWindow.window
            property var scr: QsWindow.window?.screen
            Item { id: deepA; property var win: QsWindow.window }
        }
        // Upstream's QsWindow.mapFromItem/itemPosition/itemRect: inir's bar popups center on
        // their hover target with them and fell back to the left edge without.
        Item {
            x: 10; y: 20
            Item { id: target; x: 5; y: 7; width: 30; height: 12 }
            // Not an item: QsWindow answers for the closest item parent (inir's StyledPopup).
            LazyLoader { id: loaderProbe }
        }
    }
    PanelWindow {
        id: panel
        screen: Quickshell.screens[0]
        anchors.top: true
        implicitHeight: 30
        Item { id: probeB; property var scr: QsWindow.window?.screen }
    }
    // inir's bar: a PanelWindow built by a LazyLoader, its content reading the screen.
    LazyLoader {
        id: lazy
        active: true
        component: PanelWindow {
            screen: Quickshell.screens[0]
            anchors.bottom: true
            implicitHeight: 30
            property alias probe: probeD
            Item { Item { id: probeD; property var scr: QsWindow.window?.screen } }
        }
    }
    FloatingWindow {
        id: noScreen
        implicitWidth: 100; implicitHeight: 100
        Item { id: probeC; property var scr: QsWindow.window?.screen }
    }

    Timer {
        interval: 500
        running: true
        onTriggered: {
            const s0 = Quickshell.screens[0]
            root.check("has a screen", !!s0)
            root.check("window binding", probeA.win === withScreen)
            root.check("nested window binding", deepA.win === withScreen)
            root.check("screen binding", probeA.scr === s0)
            root.check("panel screen binding", probeB.scr === s0)
            const p = probeA.QsWindow.mapFromItem(target, 2, 3)
            root.check("attached mapFromItem x,y", p.x === 17 && p.y === 30)
            const q = probeA.QsWindow.mapFromItem(target, Qt.point(1, 1))
            root.check("attached mapFromItem point", q.x === 16 && q.y === 28)
            const r = probeA.QsWindow.mapFromItem(target, 0, 0, 4, 5)
            root.check("attached mapFromItem rect", r.x === 15 && r.y === 27 && r.width === 4 && r.height === 5)
            const ip = probeA.QsWindow.itemPosition(target), ir = probeA.QsWindow.itemRect(target)
            root.check("attached itemPosition/itemRect", ip.x === 15 && ip.y === 27 && ir.width === 30 && ir.height === 12)
            const lp = loaderProbe.QsWindow?.mapFromItem(target, 2, 3)
            root.check("non-item attaches to its item", loaderProbe.QsWindow?.window === withScreen && lp?.x === 17 && lp?.y === 30)
            const wp = withScreen.mapFromItem(target, 2, 3), wr = withScreen.itemRect(target)
            root.check("window mapFromItem/itemRect", wp.x === 17 && wp.y === 30 && wr.x === 15 && wr.width === 30)
            root.check("lazy panel screen binding", lazy.item?.probe.scr === s0)
            root.check("unset screen reads its screen", noScreen.screen === s0)
            root.check("unset screen binding", probeC.scr === s0)
            root.finish()
        }
    }
}
