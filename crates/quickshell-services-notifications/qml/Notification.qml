import QtQml
import Quickshell
import Quickshell.Services.Notifications

// One notification, created and owned by NotificationServer. `tracked` mirrors upstream's
// close reason: a notification is tracked while its close reason is 0.
NotificationBase {
    id: n
    property int id
    property bool tracked: false
    // Carried over from the last generation by a reload (keepOnReload).
    property bool lastGeneration: false
    property real expireTimeout
    property string appName
    property string appIcon
    property string summary
    property string body
    property int urgency: NotificationUrgency.Normal
    property list<NotificationAction> actions
    property bool hasActionIcons
    property bool resident
    property string desktopEntry
    property string image
    property bool hasInlineReply
    property string inlineReplyPlaceholder
    property var hints: ({})

    signal closed(int reason)

    function expire() { _close(NotificationCloseReason.Expired) }
    function dismiss() { _close(NotificationCloseReason.Dismissed) }
    function sendInlineReply(replyText) {
        if (!_server.inlineReplySupported) return console.error("Inline reply support disabled on server")
        if (!hasInlineReply) return console.error("Cannot send reply to notification without inline-reply action")
        if (_gone) return console.error("Cannot send reply to destroyed notification", n)
        NotificationServerCore.reply(id, replyText)
        if (!resident) _close(NotificationCloseReason.Dismissed)
    }

    property QtObject _server
    property int _closeReason: NotificationCloseReason.Dismissed
    property bool _gone: false
    property bool _closing: false
    property Component _actionComponent: Component { NotificationAction {} }

    onTrackedChanged: if (!_closing) _close(tracked ? 0 : NotificationCloseReason.Dismissed)

    function _close(reason) {
        if (_gone) return console.error("Cannot close destroyed notification", n)
        _closeReason = reason
        _closing = true
        tracked = reason === 0
        _closing = false
        if (reason !== 0) _server._delete(n, reason)
    }

    function _update(d) {
        const h = d.hints
        if (d.lastGeneration) lastGeneration = true
        expireTimeout = d.expireTimeout
        appName = d.appName
        summary = d.summary
        body = d.body
        hasActionIcons = !!h["action-icons"]
        resident = !!h["resident"]
        n["transient"] = !!h["transient"]
        desktopEntry = h["desktop-entry"] ?? ""
        urgency = h["urgency"] ?? NotificationUrgency.Normal
        appIcon = d.appIcon
        let img = d.image
        if (!img) {
            img = h["image-path"] ?? h["image_path"] ?? ""
            if (img && !img.startsWith("file:")) img = Quickshell.iconPath(img)
        }
        image = img
        hints = h

        const a = d.actions
        if (a.length % 2 !== 0) return console.warn(n, "(" + appName + ") sent an action set of an invalid length.")
        let list = [], changed = false, dead = []
        for (let i = 0; i < a.length; i += 2) {
            if (a[i] === "inline-reply" && _server.inlineReplySupported) {
                if (hasInlineReply) console.warn(n, "(" + appName + ") sent an action set with duplicate inline-reply actions.")
                else { hasInlineReply = true; inlineReplyPlaceholder = a[i + 1] }
                continue
            }
            const old = actions[list.length]
            if (old && old.identifier === a[i]) { old.text = a[i + 1]; list.push(old); continue }
            if (old) dead.push(old)
            list.push(_actionComponent.createObject(n, { identifier: a[i], text: a[i + 1], _notification: n }))
            changed = true
        }
        for (let i = list.length; i < actions.length; i++) { if (!dead.includes(actions[i])) dead.push(actions[i]); changed = true }
        if (changed) actions = list
        for (const x of dead) x.destroy()
    }
}
