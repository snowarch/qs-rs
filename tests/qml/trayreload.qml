import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.SystemTray

// The tray outlives a reload, as upstream's StatusNotifierWatcher::instance(): the new
// generation gets the registered items back at once, and the watcher name is still served.
ShellRoot {
    id: root
    FileView { id: marker; path: Quickshell.statePath("trayreload-generation"); blockLoading: true; printErrors: false }
    readonly property int generation: parseInt(marker.text() || "0")
    property string reply: ""
    Process {
        id: sh
        stdout: StdioCollector { onStreamFinished: root.reply = text.trim() }
    }
    property int ticks: 0
    Timer {
        interval: 100; running: true; repeat: true
        onTriggered: {
            root.ticks++
            const items = SystemTray.items.values
            if (root.generation === 0) {
                if (items.length >= 2 && items.every(i => i.id)) {
                    marker.setText(String(items.length))
                    Quickshell.reload(false)
                    running = false
                }
            } else if (items.length === root.generation && items.every(i => i.id)) {
                if (!root.reply) {
                    sh.command = ["sh", "-c", "gdbus call --session -d org.kde.StatusNotifierWatcher -o /StatusNotifierWatcher -m org.freedesktop.DBus.Properties.Get org.kde.StatusNotifierWatcher RegisteredStatusNotifierItems; gdbus call --session -d org.freedesktop.DBus -o /org/freedesktop/DBus -m org.freedesktop.DBus.ListNames"]
                    if (!sh.running) sh.running = true
                    return
                }
                // One host: the last generation's host does not linger on the bus.
                const hosts = (root.reply.match(/org\.kde\.StatusNotifierHost-/g) ?? []).length
                const ok = root.reply.includes("StatusNotifierItem-4242-1/StatusNotifierItem") && root.reply.includes("pathonly") && hosts === 1
                if (!ok) console.log("FAILED: hosts=" + hosts + " " + root.reply)
                Qt.exit(ok ? 0 : 1)
            } else if (root.ticks > 30) {
                console.log("FAILED: items after reload " + JSON.stringify(Array.from(items).map(i => i.id)))
                Qt.exit(1)
            }
        }
    }
}
