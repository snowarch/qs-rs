import QtQuick
import Quickshell
import Quickshell.Wayland

// Driven by tests/turnlayer/wrap.sh in niri: a dock that turns from the bottom edge to the left one at 1.5 s. The
// axis along the edge was the compositor's and now is its own, and the size the dock asks for there is 0 for a
// moment. Its input mask and blur follow the strip, as inir's dock does. The screenshot at 4 s shows it drawn whole.
ShellRoot {
    property bool side: false
    Timer { interval: 1500; running: true; onTriggered: side = true }
    PanelWindow {
        id: dock
        WlrLayershell.namespace: "qsrs:turn"
        WlrLayershell.layer: WlrLayer.Top
        anchors { bottom: true; top: side; left: true; right: !side }
        implicitWidth: side ? 60 : 0
        implicitHeight: side ? 0 : 60
        color: "transparent"
        exclusiveZone: 60
        mask: Region { item: strip }
        BackgroundEffect.blurRegion: Region { item: strip }
        Rectangle {
            id: strip
            anchors.fill: parent
            color: "red"
        }
    }
}
