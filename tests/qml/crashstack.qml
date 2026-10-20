import QtQuick
import Quickshell
import Quickshell.Io
import "teardownlib"

// Driven by tests/crashstack/wrap.sh, which kills it or lets it crash and reads the QML stack
// the fatal signal handler prints. CRASH_CASE: spin (JS busy in spin()), idle (no JS running),
// teardown (a real SEGV inside the old generation's JS while a reload tears it down).
ShellRoot {
    id: root
    readonly property string mode: Quickshell.env("CRASH_CASE") ?? ""
    FileView { id: marker; path: Quickshell.statePath("crash-generation"); blockLoading: true; printErrors: false }
    readonly property int generation: parseInt(marker.text() || "0")
    readonly property int sum: Holder1.seen + Holder2.seen + Holder3.seen + Holder4.seen + Holder5.seen + Holder6.seen

    function spin() {
        console.log("SPINNING")
        let n = 0
        while (true) n++
    }

    Timer {
        interval: 200; running: true
        onTriggered: {
            if (root.mode === "spin") root.spin()
            else if (root.mode === "idle") console.log("IDLE")
            else if (root.mode === "teardown" && root.generation < 3) {
                marker.setText(String(root.generation + 1))
                Quickshell.reload(false)
            }
        }
    }
}
