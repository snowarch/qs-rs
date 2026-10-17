import QtQuick
import Quickshell
import Quickshell.Wayland

// Exclusive zones follow upstream: writing exclusiveZone switches the mode to Normal, so inir's
// bars (exclusionMode: Ignore next to an exclusiveZone binding) reserve their space.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) errors.push(name) }
    property int zone: 30

    // modules/ii/barM3/M3Bar.qml:89-90 and modules/ii/bar/Bar.qml:141-144
    PanelWindow {
        id: bar; implicitHeight: 40
        anchors { top: true; left: true; right: true }
        exclusionMode: ExclusionMode.Ignore
        exclusiveZone: root.zone
    }
    PanelWindow { id: auto; implicitHeight: 40; anchors { top: true; left: true; right: true } margins.bottom: 5 }
    PanelWindow { id: zero; implicitHeight: 40; anchors { top: true; left: true; right: true } exclusiveZone: 0 }
    PanelWindow { id: ignore; implicitHeight: 40; anchors { top: true; left: true; right: true } exclusionMode: ExclusionMode.Ignore }
    PanelWindow { id: attached; implicitHeight: 40; anchors.bottom: true; WlrLayershell.exclusiveZone: 12 }

    Timer {
        interval: 100; running: true
        onTriggered: {
            root.check("ignore+zone.reserves", bar.exclusionMode === ExclusionMode.Normal && bar._exclusiveZone === 30)
            root.zone = 44
            root.check("zone.follows", bar._exclusiveZone === 44)
            bar.exclusionMode = ExclusionMode.Ignore
            root.check("mode.after.zone", bar._exclusiveZone === -1)
            root.zone = 50
            root.check("zone.after.mode", bar._exclusiveZone === 50)
            root.check("auto", auto.exclusionMode === ExclusionMode.Auto && auto._exclusiveZone === 45)
            root.check("explicit.zero", zero.exclusionMode === ExclusionMode.Normal && zero._exclusiveZone === 0)
            root.check("ignore", ignore._exclusiveZone === -1)
            root.check("attached", attached.exclusiveZone === 12 && attached._exclusiveZone === 12)
            if (root.errors.length) console.log("FAILED: " + root.errors.join(", "))
            Qt.exit(root.errors.length ? 1 : 0)
        }
    }
}
