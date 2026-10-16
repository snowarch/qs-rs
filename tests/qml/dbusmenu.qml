import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.SystemTray

// Tray menus against the dbusmenu of tests/systemtray/mock.py: QsMenuOpener loads the item's
// menu, entries carry label/separator/toggle/submenu/icon, hidden items are filtered, submenus
// open through the entry, LayoutUpdated and partial ItemsPropertiesUpdated apply, and
// opened/clicked/closed events reach the app.
ShellRoot {
    id: root
    property var errors: []
    property int step: 0
    property int ticks: 0
    property var reply: null
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }
    readonly property SystemTrayItem item: SystemTray.items.values.find(i => i.id === "pathonly") ?? null
    function texts(model) { return model.values.map(e => e.isSeparator ? "-" : e.text).join(",") }
    function entry(text) { return opener.children.values.find(e => e.text === text) ?? null }

    QsMenuOpener { id: opener }
    QsMenuOpener { id: sub }
    Repeater { id: rows; model: opener.children; Item { required property QsMenuEntry modelData } }

    Process {
        id: sh
        stdout: StdioCollector { onStreamFinished: root.reply = text.trim() }
    }
    function mock(method) {
        reply = null
        sh.command = ["gdbus", "call", "--session", "-d", "org.qsrs.Mock", "-o", "/org/qsrs/Mock", "-m", "org.qsrs.Mock." + method]
        sh.running = true
    }

    Timer {
        interval: 50
        repeat: true
        running: true
        onTriggered: {
            root.ticks++
            if (root.ticks > 180) { root.check("timeout at step " + root.step + " " + root.texts(opener.children), false); root.finish(); return }
            switch (root.step) {
            case 0:
                if (!root.item) return
                root.check("menu handle", root.item.hasMenu && root.item.menu !== null)
                root.check("empty before load", opener.children.values.length === 0)
                opener.menu = root.item.menu
                root.step = 1
                break
            case 1:
                if (opener.children.values.length === 0) return
                root.check("entries " + root.texts(opener.children), root.texts(opener.children) === "Open,-,Check,Sub")
                root.check("repeater rows", rows.count === 4)
                const open = root.entry("Open"), check = root.entry("Check"), s = root.entry("Sub")
                root.check("open", open.enabled && !open.hasChildren && open.buttonType === QsMenuButtonType.None)
                root.check("check", check.buttonType === QsMenuButtonType.CheckBox && check.checkState === Qt.Checked)
                root.check("sub " + s.icon, s.hasChildren && s.icon.startsWith("image://icon/folder"))
                root.check("root has children", opener.menu.menu.hasChildren)
                sub.menu = s
                root.step = 2
                break
            case 2:
                // Opening Sub makes the app add "Late" and send LayoutUpdated for it.
                if (root.texts(sub.children) !== "Inner,Late") return
                root.entry("Check").triggered()
                root.step = 3
                break
            case 3:
                const c = root.entry("Check")
                if (c.checkState !== Qt.Unchecked) return
                root.check("partial update keeps label " + c.text, c.text === "Check" && c.buttonType === QsMenuButtonType.CheckBox)
                root.mock("ShowHidden")
                root.step = 4
                break
            case 4:
                if (root.texts(opener.children) !== "Open,-,Check,Sub,Hidden") return
                root.mock("DropIcon")
                root.step = 5
                break
            case 5:
                if (root.entry("Sub").icon !== "") return
                sub.menu = null
                opener.menu = null
                root.check("closed menu has no children", opener.children.values.length === 0)
                root.mock("Calls")
                root.step = 6
                break
            case 6:
                if (root.reply === null) return
                for (const want of ["menu AboutToShow 0", "menu opened 0", "menu opened 4", "menu clicked 3", "menu closed 4"])
                    root.check(`call ${want} in ${root.reply}`, root.reply.includes(want))
                root.step = 7
                root.ticks = 170
                break
            case 7:
                // `closed` for the root is sent after the opener let go.
                if (root.reply === null) return
                if (!root.reply.includes("menu closed 0")) { if (sh.running) return; root.mock("Calls"); return }
                root.finish()
            }
        }
    }
}
