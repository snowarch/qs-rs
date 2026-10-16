import QtQml
import Quickshell.Services.Notifications

QtObject {
    property string identifier
    property string text
    property QtObject _notification

    function invoke() {
        const n = _notification
        if (n._gone) return console.error("Cannot invoke destroyed notification", n)
        NotificationServerCore.invokeAction(n.id, identifier)
        if (!n.resident) n._close(NotificationCloseReason.Dismissed)
    }
}
