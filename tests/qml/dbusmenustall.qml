import QtQuick
import Quickshell
import Quickshell.Services.SystemTray

// A tray app whose menu never answers a property read (shelly-notifications: IconThemePath) must not hold back
// the menus of the others. The stuck item's menu is opened first, so its connect is the first job queued.
ShellRoot {
    id: root
    property var errors: []
    property int step: 0
    property int ticks: 0
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }
    function byId(id) { return SystemTray.items.values.find(i => i.id === id) ?? null }
    function texts(model) { return model.values.map(e => e.text).join(",") }

    QsMenuOpener { id: stuck }
    QsMenuOpener { id: normal }

    Timer {
        interval: 50
        repeat: true
        running: true
        onTriggered: {
            root.ticks++
            if (root.ticks > 120) {
                root.check("timeout at step " + root.step + ": stuck=[" + root.texts(stuck.children) + "] normal=[" + root.texts(normal.children) + "]", false)
                root.finish()
                return
            }
            switch (root.step) {
            case 0:
                if (!root.byId("stuck") || !root.byId("normal")) return
                stuck.menu = root.byId("stuck").menu
                root.step = 1
                break
            case 1:
                normal.menu = root.byId("normal").menu
                root.step = 2
                break
            case 2:
                if (normal.children.values.length === 0) return
                root.check("normal entries " + root.texts(normal.children), root.texts(normal.children) === "Normal entry")
                root.finish()
            }
        }
    }
}
