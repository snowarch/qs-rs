import QtQuick
import Quickshell
import Quickshell.Wayland

// Driven by tests/livelayer/wrap.sh in a headless KWin: layer surface state that changes on a
// window that never redraws (inir's reservations) still reaches the compositor. At 1.5 s the
// reservation takes 300 px and the red bar moves 100 px in; the screenshot at 4 s shows both.
ShellRoot {
    property bool late: false
    Timer { interval: 1500; running: true; onTriggered: late = true }
    PanelWindow {
        WlrLayershell.layer: WlrLayer.Background
        anchors.top: true
        implicitWidth: 1; implicitHeight: 1
        color: "transparent"
        exclusionMode: ExclusionMode.Normal
        exclusiveZone: late ? 300 : 0
        mask: Region {}
    }
    PanelWindow {
        anchors { top: true; left: true; right: true }
        margins.left: late ? 100 : 0
        implicitHeight: 60
        color: "red"
        exclusiveZone: 0
    }
}
