import QtQuick
import Quickshell
import Quickshell.Widgets

// ClippingRectangle's border value type and IconImage sizing. The offscreen platform has no
// shaders, so the clipping itself is checked visually under scripts/nested.sh.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }

    FloatingWindow {
        id: win
        implicitWidth: 120
        implicitHeight: 60
        color: "black"

        ClippingRectangle {
            id: clip
            x: 0; y: 0; width: 60; height: 60
            radius: 30
            color: "blue"
            border.width: 4
            border.color: "#00ff00"
            Rectangle { anchors.fill: parent; color: "red" }
        }
        IconImage {
            id: icon
            x: 70; y: 10
            implicitSize: 40
            source: Quickshell.iconPath("folder")
        }
    }

    Timer {
        interval: 800
        running: true
        onTriggered: {
            root.check("border value type", clip.border.width === 4 && clip.border.pixelAligned)
            root.check("icon size", icon.width === 40 && icon.actualSize === 40 && icon.backer.sourceSize.width === 40)
            root.check("icon loads", icon.status === Image.Ready)
            root.check("clip content", clip.contentItem.children.length === 1 && clip.children[0].color === Qt.color("red"))
            root.finish()
        }
    }
}
