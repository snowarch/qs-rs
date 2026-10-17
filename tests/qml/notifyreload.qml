import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Notifications

// The notification service outlives a reload, as upstream's NotificationServer::instance(): the
// new generation gets the tracked notifications back (lastGeneration) and the name stays served,
// with ids still counting up.
ShellRoot {
    id: root
    FileView { id: marker; path: Quickshell.statePath("notifyreload-generation"); blockLoading: true; printErrors: false }
    readonly property int generation: parseInt(marker.text() || "0")
    property string reply: ""

    NotificationServer {
        id: server
        onNotification: n => n.tracked = true
    }
    Process {
        id: sh
        stdout: StdioCollector { onStreamFinished: root.reply = text.trim() }
    }
    function notify(summary) {
        reply = ""
        sh.command = ["gdbus", "call", "--session", "-d", "org.freedesktop.Notifications", "-o", "/org/freedesktop/Notifications",
            "-m", "org.freedesktop.Notifications.Notify", "app", "0", "icon", summary, "body", "[]", "{}", "5000"]
        sh.running = true
    }
    function idOf(r) { return Number((/uint32 (\d+)/.exec(r ?? "") ?? [0, 0])[1]) }

    property int step: 0
    property int firstId: 0
    Timer {
        interval: 100; running: true; repeat: true
        onTriggered: {
            const tracked = Array.from(server.trackedNotifications.values)
            if (root.generation === 0) {
                if (root.step === 0) { root.notify("keep me"); root.step = 1 }
                else if (root.step === 1 && tracked.length === 1 && root.reply) {
                    marker.setText(String(root.idOf(root.reply)))
                    root.step = 2
                    Quickshell.reload(false)
                }
            } else if (root.step === 0) {
                const n = tracked[0]
                if (!n) return
                root.firstId = root.generation
                const ok = tracked.length === 1 && n.summary === "keep me" && n.lastGeneration && n.id === root.firstId
                if (!ok) { console.log("FAILED: carried " + JSON.stringify(tracked.map(x => [x.id, x.summary, x.lastGeneration]))); Qt.exit(1) }
                root.notify("after reload")
                root.step = 1
            } else if (root.step === 1 && root.reply) {
                const ok = root.idOf(root.reply) > root.firstId && tracked.length === 2 && !tracked[1].lastGeneration
                if (!ok) console.log("FAILED: after reload reply=" + root.reply + " tracked=" + tracked.length)
                Qt.exit(ok ? 0 : 1)
            }
        }
    }
}
