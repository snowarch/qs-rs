import QtQuick
import QtQuick.Window as QW
import Quickshell

// One lock surface; WlSessionLock (include/sessionlock.h) creates one per screen, picks its
// screen and shows its window with the lock surface role.
QtObject {
    id: root
    default property alias data: _window.data
    readonly property Item contentItem: _window.contentItem
    readonly property bool visible: _window.visible
    readonly property int width: _window.width
    readonly property int height: _window.height
    readonly property QtObject screen: Quickshell.screens.find(s => s.name === _window.screen?.name) ?? null
    property color color: "white"

    readonly property QW.Window _qsLockWindow: QW.Window {
        id: _window
        color: root.color
        visible: false
    }
}
