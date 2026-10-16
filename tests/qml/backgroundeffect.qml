import QtQuick
import Quickshell
import Quickshell.Wayland

// BackgroundEffect attaches to windows, keeps its region and
// follows the region's lifetime. The blur itself needs a compositor (checked live).
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) errors.push(name) }

    PanelWindow {
        id: panel
        implicitWidth: 100; implicitHeight: 40
        color: "transparent"
        BackgroundEffect.blurRegion: Region { item: box; radius: 8 }
        Rectangle { id: box; width: 50; height: 20 }
    }

    FloatingWindow { id: floating; implicitWidth: 50; implicitHeight: 50 }

    Component { id: regionComponent; Region { width: 10; height: 10 } }

    Timer {
        interval: 100; running: true
        onTriggered: {
            root.check("declared", panel.BackgroundEffect.blurRegion !== null && panel.BackgroundEffect.blurRegion.item === box)
            const r = regionComponent.createObject(root)
            floating.BackgroundEffect.blurRegion = r
            root.check("assigned", floating.BackgroundEffect.blurRegion === r)
            r.destroy()
            Qt.callLater(() => {
                root.check("regionDestroyed", floating.BackgroundEffect.blurRegion === null)
                if (root.errors.length) console.log("FAILED: " + root.errors.join(", "))
                Qt.exit(root.errors.length ? 1 : 0)
            })
        }
    }
}
