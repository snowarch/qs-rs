import QtQuick
import Quickshell
import Quickshell.Wayland

// Needs a compositor with zwlr_foreign_toplevel_manager_v1 (niri):
//   scripts/nested-niri.sh /tmp/x.png 6 qs-rs -p tests/nested/toplevel.qml   (prints PASS/FAIL)
// Our own windows show up with title, appId and screen; the new window is active and
// activate() moves focus back; fullscreen goes through the compositor; close() closes the
// window, emits closed and removes the toplevel.
ShellRoot {
    id: root
    property var log: []
    property int step: 0

    function find(title) { return ToplevelManager.toplevels.values.find(t => t.title === title) ?? null }
    function check(name, ok) { if (!ok) log.push("failed: " + name) }

    FloatingWindow { id: a; title: "qs-a"; implicitWidth: 300; implicitHeight: 200; color: "teal" }
    FloatingWindow { id: b; title: "qs-b"; visible: root.step >= 1; implicitWidth: 300; implicitHeight: 200; color: "orange" }

    property Toplevel closedOne: null
    property bool sawClosed: false
    Connections {
        target: root.closedOne
        function onClosed() { root.sawClosed = true }
    }

    Timer {
        interval: 250
        repeat: true
        running: true
        property int ticks: 0
        onTriggered: {
            ticks++
            const ta = root.find("qs-a"), tb = root.find("qs-b")
            const active = ToplevelManager.activeToplevel
            if (root.step === 0 && ta && active === ta) {
                root.check("appId", ta.appId.length > 0)
                root.check("screens", ta.screens.length === 1 && ta.screens[0] === Quickshell.screens[0])
                root.check("not fullscreen", !ta.fullscreen && !ta.minimized)
                root.check("no parent", ta.parent === null)
                root.log.push("a")
                root.step = 1
            } else if (root.step === 1 && tb && active === tb) {
                root.check("a deactivated", !ta.activated && tb.activated)
                root.log.push("b")
                ta.activate()
                root.step = 2
            } else if (root.step === 2 && active === ta) {
                root.log.push("activate")
                ta.fullscreen = true
                root.check("fullscreen waits for the compositor", !ta.fullscreen)
                root.step = 3
            } else if (root.step === 3 && ta.fullscreen) {
                root.log.push("fullscreen")
                root.closedOne = tb
                tb.close()
                root.step = 4
            } else if (root.step === 4 && !tb && root.sawClosed) {
                root.check("one left", ToplevelManager.toplevels.values.length === 1)
                root.check("window closed", !b.visible)
                root.log.push("close")
                root.step = 5
            }
            if (root.step === 5 || ticks > 20) {
                const ok = JSON.stringify(root.log) === '["a","b","activate","fullscreen","close"]'
                console.log(ok ? "PASS" : "FAIL step " + root.step + " " + JSON.stringify(root.log))
                Qt.exit(ok ? 0 : 1)
            }
        }
    }
}
