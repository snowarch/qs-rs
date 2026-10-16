import QtQml

// Global shortcut through hyprland-global-shortcuts-v1. Without that protocol (e.g. niri) the
// shortcut is registered nowhere and never fires.
QtObject {
    property string appid: "quickshell"
    property string name
    property string description
    property string triggerDescription
    readonly property bool pressed: false
    signal pressed()
    signal released()
}
