import QtQml
import Quickshell
import Quickshell.Services.Notifications

// Serves org.freedesktop.Notifications through NotificationServerCore. Notifications are kept
// by id; one that is not tracked after `notification` fires is dropped right away.
QtObject {
    id: root
    property bool keepOnReload: true
    property bool persistenceSupported: false
    property bool bodySupported: true
    property bool bodyMarkupSupported: false
    property bool bodyHyperlinksSupported: false
    property bool bodyImagesSupported: false
    property bool actionsSupported: false
    property bool actionIconsSupported: false
    property bool imageSupported: false
    property bool inlineReplySupported: false
    property list<string> extraHints
    readonly property QtObject trackedNotifications: ObjectModel {}

    signal notification(Notification notification)

    readonly property string _capabilities: {
        const c = []
        if (persistenceSupported) c.push("persistence")
        if (bodySupported) {
            c.push("body")
            if (bodyMarkupSupported) c.push("body-markup")
            if (bodyHyperlinksSupported) c.push("body-hyperlinks")
            if (bodyImagesSupported) c.push("body-images")
        }
        if (actionsSupported) {
            c.push("actions")
            if (actionIconsSupported) c.push("action-icons")
        }
        if (imageSupported) c.push("icon-static")
        if (inlineReplySupported) c.push("inline-reply")
        return c.concat(Array.from(extraHints)).join(" ")
    }
    property bool _live: false
    property var _byId: ({})
    property Component _component: Component { Notification {} }

    on_CapabilitiesChanged: if (_live) NotificationServerCore.start(_capabilities)
    Component.onCompleted: { _live = true; NotificationServerCore.start(_capabilities) }

    function _delete(n, reason) {
        if (_byId[n.id] !== n) return
        n.closed(reason)
        delete _byId[n.id]
        trackedNotifications.values = Array.from(trackedNotifications.values).filter(x => x !== n)
        NotificationServerCore.closed(n.id, reason)
        n._gone = true
        n.destroy()
    }

    property Connections _core: Connections {
        target: NotificationServerCore
        function onNotified(id, json) {
            const d = JSON.parse(json)
            const old = root._byId[id]
            if (old) return old._update(d)
            const n = root._component.createObject(root, { id: id, _server: root })
            n._update(d)
            root.notification(n)
            if (!n.tracked) {
                NotificationServerCore.closed(id, n._closeReason)
                n._gone = true
                n.destroy()
            } else {
                root._byId[id] = n
                root.trackedNotifications.values = [...root.trackedNotifications.values, n]
            }
        }
        function onCloseRequested(id) {
            const n = root._byId[id]
            if (n) root._delete(n, NotificationCloseReason.CloseRequested)
        }
    }
}
