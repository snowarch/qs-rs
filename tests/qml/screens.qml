import QtQuick
import Quickshell
import Quickshell.Wayland

// Driven by tests/screens/wrap.sh in a headless KWin with two outputs: every per-screen panel
// lands on its own output, and one moved to the second output while shown follows it there.
ShellRoot {
    Variants {
        model: Quickshell.screens
        PanelWindow {
            required property var modelData
            screen: modelData
            anchors { top: true; left: true; right: true }
            implicitHeight: 60
            color: modelData.name === Quickshell.screens[0].name ? "red" : "lime"
            exclusiveZone: 0
        }
    }
    PanelWindow {
        id: mover
        screen: Quickshell.screens[0]
        anchors { bottom: true; left: true; right: true }
        implicitHeight: 60
        color: "blue"
        exclusiveZone: 0
        Timer { interval: 1500; running: true; onTriggered: mover.screen = Quickshell.screens[1] }
    }
}
