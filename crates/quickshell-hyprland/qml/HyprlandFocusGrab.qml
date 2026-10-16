import QtQml

// Hyprland focus grab. Inactive without hyprland-focus-grab-v1: activating clears immediately.
QtObject {
    id: root
    property bool active: false
    property list<QtObject> windows
    signal cleared()
    onActiveChanged: if (active) Qt.callLater(() => { root.active = false; root.cleared() })
}
