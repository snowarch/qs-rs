import QtQuick
import Quickshell
import Quickshell.Networking
import Quickshell.Services.Pipewire

// A list read from JS outlives its owner: inir builds models out of toplevels, notifications and
// Variants, and the window or notification can be destroyed while that JS value is still around.
// With list<QtObject> properties Qt read the dead owner and segfaulted the live shell (2026-10-19,
// a game launch closing dozens of windows); the lists are JS arrays now, so this has to survive.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) errors.push(name) }

    property var held: null
    property int tick: 0

    LazyLoader {
        id: holder
        active: true
        Variants {
            model: ["a", "b"]
            QtObject { required property string modelData }
        }
    }

    // Lists of objects that go away while the shell runs: a vanished network, a tracker whose
    // delegate is gone.
    property var heldSettings: null
    property var heldGroups: null
    Component { id: networkComp; Network { property QtObject s: QtObject {} Component.onCompleted: nmSettings = [s] } }
    Component { id: trackerComp; PwNodeLinkTracker {} }

    // The 03:59 stack: a binding re-evaluates a model that wraps the held list in a JS object.
    Repeater {
        model: { root.tick; return root.held ? [{ list: root.held }] : [] }
        Item {}
    }

    Timer {
        interval: 50
        running: true
        onTriggered: {
            root.held = holder.item.instances
            root.check("instances is a list", root.held.length === 2)
            root.check("screens has map", typeof Quickshell.screens.map === "function")
            const network = networkComp.createObject(null)
            const tracker = trackerComp.createObject(null)
            root.heldSettings = network.nmSettings
            root.heldGroups = tracker.linkGroups
            network.destroy()
            tracker.destroy()
            holder.active = false
            again.start()
        }
    }

    Timer {
        id: again
        interval: 20
        repeat: true
        property int n: 0
        onTriggered: {
            gc()
            root.tick++
            if (++n < 15) return
            root.check("nmSettings survives its network", root.heldSettings.length === 1)
            root.check("linkGroups survives its tracker", root.heldGroups.length === 0)
            stop()
            if (root.errors.length) {
                console.error("FAIL", root.errors.join(", "))
                Qt.exit(1)
            } else {
                Qt.exit(0)
            }
        }
    }
}
