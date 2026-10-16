import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.SystemTray

// Runs against tests/systemtray/mock.py: our watcher registers both items (by service and by
// bare path), the host reads them, icons resolve to theme names or pixmap files, method calls
// reach the item, New* signals re-read it, and an item leaves when its name goes away.
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
    function byId(id) { return SystemTray.items.values.find(i => i.id === id) ?? null }
    readonly property SystemTrayItem pathonly: byId("pathonly")
    Image { id: img; source: root.pathonly ? root.pathonly.icon : "" }

    Process {
        id: sh
        stdout: StdioCollector { onStreamFinished: root.reply = text.trim() }
    }
    function bus(dest, path, method, args) {
        reply = null
        sh.command = ["gdbus", "call", "--session", "-d", dest, "-o", path, "-m", method].concat(args)
        sh.running = true
    }
    function mock(method, args) { bus("org.qsrs.Mock", "/org/qsrs/Mock", "org.qsrs.Mock." + method, args ?? []) }

    Timer {
        interval: 50
        repeat: true
        running: true
        onTriggered: {
            root.ticks++
            if (root.ticks > 180) { root.check("timeout at step " + root.step, false); root.finish(); return }
            const items = SystemTray.items.values
            switch (root.step) {
            case 0:
                if (items.length < 2) return
                const n = root.byId("named"), p = root.pathonly
                root.check("enums", Status.NeedsAttention === 2 && Category.Communications === 3)
                root.check("named props", n.title === "Named" && n.status === Status.Active && n.category === Category.Communications)
                root.check("tooltip", n.tooltipTitle === "Tip" && n.tooltipDescription === "Tip text")
                root.check("named icon " + n.icon, n.icon === "image://icon/folder")
                root.check("menus", !n.hasMenu && p.hasMenu && !p.onlyMenu)
                root.check("pixmap icon " + p.icon, p.icon.startsWith("file://") && p.icon.endsWith(".bmp"))
                p.activate()
                n.secondaryActivate()
                p.scroll(3, true)
                root.step = 1
                break
            case 1:
                if (img.status === Image.Loading || img.status === Image.Null) return
                root.check("pixmap loads", img.status === Image.Ready && img.sourceSize.width === 2 && img.sourceSize.height === 1)
                root.bus("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher", "org.freedesktop.DBus.Properties.Get",
                    ["org.kde.StatusNotifierWatcher", "RegisteredStatusNotifierItems"])
                root.step = 2
                break
            case 2:
                if (root.reply === null) return
                root.check("watcher items " + root.reply, /'org\.kde\.StatusNotifierItem-4242-1\/StatusNotifierItem'/.test(root.reply)
                    && /':1\.\d+\/org\/ayatana\/NotificationItem\/pathonly'/.test(root.reply))
                root.mock("Change", ["New title", "NeedsAttention"])
                root.step = 3
                break
            case 3:
                if (root.pathonly.title !== "New title" || root.pathonly.status !== Status.NeedsAttention) return
                root.check("attention falls back to icon pixmap", root.pathonly.icon.endsWith(".bmp"))
                root.mock("DropNamed")
                root.step = 4
                break
            case 4:
                if (items.length !== 1) return
                root.check("named removed", root.byId("named") === null && root.pathonly !== null)
                root.mock("Calls")
                root.step = 5
                break
            case 5:
                if (root.reply === null) return
                root.check("calls " + root.reply, root.reply === "(['pathonly Activate', 'named SecondaryActivate', 'pathonly Scroll 3 horizontal'],)")
                root.finish()
            }
        }
    }
}
