import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Notifications

// Serves org.freedesktop.Notifications on a private bus and drives it with gdbus: capabilities,
// properties and hints, pixmap images, replacing, actions, inline replies, untracked
// notifications, CloseNotification, expire and the signals clients receive.
ShellRoot {
    id: root
    property var errors: []
    property int step: 0
    property int ticks: 0
    property var reply: null
    property var got: []
    property var closes: []
    property Notification first: null
    property int id1: 0
    property int id2: 0
    property int id3: 0
    property int id4: 0
    property string image1: ""
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }

    NotificationServer {
        id: server
        actionsSupported: true
        imageSupported: true
        inlineReplySupported: true
        bodyMarkupSupported: true
        extraHints: ["x-test"]
        onNotification: n => {
            root.got.push(n)
            if (n.summary !== "drop") n.tracked = true
        }
    }
    Connections { target: root.first; function onClosed(reason) { root.closes.push(reason) } }
    Image { id: img; source: root.first ? root.first.image : "" }

    Process {
        id: sh
        stdout: StdioCollector { onStreamFinished: root.reply = text.trim() }
    }
    function run(cmd) { reply = null; sh.command = cmd; sh.running = true }
    function bus(dest, method, args) {
        const [d, o, i] = dest === "mock" ? ["org.qsrs.Mock", "/org/qsrs/Mock", "org.qsrs.Mock"]
            : ["org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.freedesktop.Notifications"]
        run(["gdbus", "call", "--session", "-d", d, "-o", o, "-m", i + "." + method].concat(args))
    }
    function notify(replaces, summary, actions, hints) {
        bus("server", "Notify", ["app", String(replaces), "icon", summary, "body of " + summary, actions, hints, "5000"])
    }
    function idOf(r) { return Number((/uint32 (\d+)/.exec(r ?? "") ?? [0, 0])[1]) }

    Timer {
        interval: 50
        repeat: true
        running: true
        onTriggered: {
            root.ticks++
            if (root.ticks > 180) { root.check("timeout at step " + root.step, false); root.finish(); return }
            const tracked = server.trackedNotifications.values
            switch (root.step) {
            case 0: root.bus("server", "GetCapabilities", []); root.step = 1; break
            case 1:
                if (root.reply === null) return
                if (root.reply === "") { root.step = 0; return } // name not claimed yet
                root.check("capabilities", root.reply === "(['body', 'body-markup', 'actions', 'icon-static', 'inline-reply', 'x-test'],)")
                root.bus("server", "GetServerInformation", [])
                root.step = 2
                break
            case 2:
                if (root.reply === null) return
                root.check("server info", root.reply.startsWith("('quickshell', 'quickshell', ") && root.reply.endsWith("'1.2')"))
                root.notify(0, "S1", "['default', 'Open', 'inline-reply', 'Reply here']",
                    "{'urgency': <byte 2>, 'resident': <true>, 'desktop-entry': <'foo'>, 'x-custom': <'v'>,"
                    + " 'image-data': <(2, 1, 8, true, 8, 4, [byte 255, 0, 0, 128, 0, 0, 255, 255])>}")
                root.step = 3
                break
            case 3:
                if (root.reply === null || tracked.length < 1) return
                root.id1 = root.idOf(root.reply)
                const n = tracked[0]
                root.first = n
                root.check("id", root.id1 > 0 && n.id === root.id1)
                root.check("text", n.appName === "app" && n.appIcon === "icon" && n.summary === "S1" && n.body === "body of S1")
                root.check("urgency", n.urgency === NotificationUrgency.Critical && NotificationUrgency.toString(n.urgency) === "Critical")
                root.check("flags", n.resident && !n.transient && n.desktopEntry === "foo" && n.expireTimeout === 5000)
                root.check("actions", n.actions.length === 1 && n.actions[0].identifier === "default" && n.actions[0].text === "Open")
                root.check("inline reply", n.hasInlineReply && n.inlineReplyPlaceholder === "Reply here")
                root.check("hints", n.hints["x-custom"] === "v" && n.hints["urgency"] === 2 && n.hints["image-data"] === undefined)
                root.check("image url", n.image.startsWith("file://") && n.image.endsWith(".bmp"))
                root.image1 = n.image
                root.step = 4
                break
            case 4:
                if (img.status === Image.Loading || img.status === Image.Null) return
                root.check("image loads", img.status === Image.Ready && img.sourceSize.width === 2 && img.sourceSize.height === 1)
                root.notify(root.id1, "S1b", "['default', 'Open2', 'x', 'X']", "{}")
                root.step = 5
                break
            case 5:
                if (root.reply === null || root.first.summary !== "S1b") return
                root.check("replace keeps id", root.idOf(root.reply) === root.id1 && tracked.length === 1 && root.got.length === 1)
                root.check("replace updates", root.first.actions.length === 2 && root.first.actions[0].text === "Open2"
                    && root.first.urgency === NotificationUrgency.Normal && !root.first.resident && root.first.image === "")
                root.first.resident = true // keep it through the action and reply below
                root.first.actions[0].invoke()
                root.first.sendInlineReply("hi")
                root.check("resident stays", root.first.tracked && tracked.length === 1)
                root.notify(0, "drop", "[]", "{}")
                root.step = 6
                break
            case 6:
                if (root.reply === null || root.got.length < 2) return
                root.id2 = root.idOf(root.reply)
                root.check("untracked dropped", tracked.length === 1 && root.id2 === root.id1 + 1)
                root.bus("server", "CloseNotification", [String(root.id1)])
                root.step = 7
                break
            case 7:
                if (tracked.length !== 0) return
                root.check("closed signal", root.closes.length === 1 && root.closes[0] === NotificationCloseReason.CloseRequested)
                root.notify(0, "S3", "['go', 'Go']", "{}")
                root.step = 8
                break
            case 8:
                if (root.reply === null || tracked.length !== 1) return
                root.id3 = root.idOf(root.reply)
                tracked[0].actions[0].invoke()
                root.check("action dismisses", tracked.length === 0)
                root.notify(0, "S4", "[]", "{}")
                root.step = 9
                break
            case 9:
                if (root.reply === null || tracked.length !== 1) return
                root.id4 = root.idOf(root.reply)
                tracked[0].expire()
                root.run(["sh", "-c", "sleep 0.3; test -e " + root.image1.slice(7) + " && echo kept || echo gone"])
                root.step = 10
                break
            case 10:
                if (root.reply === null) return
                root.check("image file removed", root.reply === "gone")
                root.bus("mock", "Signals", [])
                root.step = 11
                break
            case 11:
                if (root.reply === null) return
                const want = [`ActionInvoked ${root.id1} default`, `NotificationReplied ${root.id1} hi`,
                    `NotificationClosed ${root.id2} 2`, `NotificationClosed ${root.id1} 3`,
                    `ActionInvoked ${root.id3} go`, `NotificationClosed ${root.id3} 2`, `NotificationClosed ${root.id4} 1`]
                root.check("client signals " + root.reply, root.reply === "([" + want.map(s => `'${s}'`).join(", ") + "],)")
                root.finish()
            }
        }
    }
}
