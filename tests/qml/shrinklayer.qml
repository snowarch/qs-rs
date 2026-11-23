import QtQuick
import Quickshell
import Quickshell.Wayland

// Driven by tests/shrinklayer/wrap.sh in niri: a dock across the bottom edge whose width is the compositor's. At 1.5 s
// a reservation takes 100 px on the right, the compositor narrows the dock, and its strip moves to the new middle;
// nothing else changes. The screenshot at 4 s shows the strip where it is and as wide as it is, not the frame drawn
// for the old width scaled into the new one.
ShellRoot {
    property bool late: false
    Timer { interval: 1500; running: true; onTriggered: late = true }
    PanelWindow {
        WlrLayershell.layer: WlrLayer.Background
        anchors.right: true
        implicitWidth: 1; implicitHeight: 1
        color: "transparent"
        exclusionMode: ExclusionMode.Normal
        exclusiveZone: late ? 100 : 0
        mask: Region {}
    }
    PanelWindow {
        id: dock
        WlrLayershell.layer: WlrLayer.Top
        anchors { bottom: true; left: true; right: true }
        implicitWidth: 800
        implicitHeight: 60
        color: "transparent"
        exclusiveZone: 0
        mask: Region { item: strip }
        BackgroundEffect.blurRegion: Region { item: strip }
        Rectangle {
            id: strip
            x: Math.round((dock.width - width) / 2)
            width: 200
            height: 60
            color: "red"
        }
    }
}
