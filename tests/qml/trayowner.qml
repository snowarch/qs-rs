import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.SystemTray

// A tray app dies with its menu open (Steam crashing, 2026-10-20 05:13): the openers empty out
// at once, and the entries a shell kept in JS from the menu it drew stay safe to call.
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
    readonly property SystemTrayItem item: SystemTray.items.values.find(i => i.id === "pathonly") ?? null
    function find(model, text) { return model.values.find(e => e.text === text) ?? null }

    QsMenuOpener { id: opener }
    QsMenuOpener { id: sub }
    property var held: null
    property var heldInner: null

    Process { id: sh; command: ["gdbus", "call", "--session", "-d", "org.qsrs.Mock", "-o", "/org/qsrs/Mock", "-m", "org.qsrs.Mock.Die"] }

    Timer {
        interval: 50
        repeat: true
        running: true
        onTriggered: {
            root.ticks++
            if (root.ticks > 120) { root.check("timeout at step " + root.step, false); root.finish(); return }
            switch (root.step) {
            case 0:
                if (!root.item) return
                opener.menu = root.item.menu
                root.step = 1
                return
            case 1:
                if (!root.find(opener.children, "Sub")) return
                sub.menu = root.find(opener.children, "Sub")
                root.step = 2
                return
            case 2:
                if (!root.find(sub.children, "Inner")) return
                root.held = root.find(opener.children, "Open")
                root.heldInner = root.find(sub.children, "Inner")
                sh.running = true
                root.step = 3
                root.ticks = 0
                return
            case 3:
                if (root.item) return
                root.step = 4
                return
            case 4:
                console.log("AFTER opener=" + opener.children.values.length + " sub=" + sub.children.values.length
                            + " held=" + root.held + " typeof triggered=" + typeof root.held?.triggered)
                root.check("opener empties", opener.children.values.length === 0)
                root.check("submenu opener empties", sub.children.values.length === 0)
                for (const e of [root.held, root.heldInner]) {
                    try { e.triggered() } catch (err) { root.check("held entry callable: " + err, false) }
                }
                root.finish()
            }
        }
    }
}
