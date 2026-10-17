import QtQuick
import Quickshell

// QML animations step at the screen's refresh rate, not Qt's fixed 16 ms tick (which gives
// ~62 steps a second and judders on 75/144 Hz screens). wrap.sh fakes a 120 Hz screen.
ShellRoot {
    id: root
    property real value: 0
    property int steps: 0
    onValueChanged: steps++

    NumberAnimation on value {
        from: 0; to: 1000; duration: 1000
        onFinished: {
            console.log("animation steps in 1 s:", root.steps)
            Qt.exit(root.steps >= 100 ? 0 : 1)
        }
    }
}
