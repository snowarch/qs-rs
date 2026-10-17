import QtQuick

// Upstream's ScreencopyView API. Capture itself is not ported yet: on niri upstream captures a
// window only through hyprland-toplevel-export (absent), so a Toplevel never has content there
// either; screens (wlr-screencopy) stay empty here until the capture lands. Without the type,
// every file that names it fails to load (inir's whole Overview in the Waffle family).
Item {
    property QtObject captureSource: null
    property bool paintCursor: false
    property bool live: false
    readonly property bool hasContent: false
    readonly property size sourceSize: Qt.size(0, 0)
    property size constraintSize: Qt.size(0, 0)
    signal stopped()
    function captureFrame() {}
}
