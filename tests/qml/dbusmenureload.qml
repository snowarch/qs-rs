import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.SystemTray

// A reload with a tray menu open lets go of the old generation's entries without a warning: they
// were released while their context was being torn down, and their icon binding re-ran on the
// cleared handle (49 "Unable to assign [undefined] to QString" in a live session, one per entry
// per reload). wrap.sh fails on any DBusMenuItem line.
ShellRoot {
    id: root
    FileView { id: marker; path: Quickshell.statePath("dbusmenureload-generation"); blockLoading: true; printErrors: false }
    readonly property int generation: parseInt(marker.text() || "0")
    readonly property SystemTrayItem item: SystemTray.items.values.find(i => i.id === "pathonly") ?? null
    QsMenuOpener { id: opener; menu: root.item?.menu ?? null }
    property int ticks: 0
    Timer {
        interval: 100; running: true; repeat: true
        onTriggered: {
            if (++root.ticks > 80) { console.log("FAILED: timeout in generation " + root.generation); Qt.exit(1) }
            if (opener.children.values.length === 0) return
            if (root.generation === 0) {
                console.log("menu open with " + opener.children.values.length + " entries, reloading")
                marker.setText("1")
                Quickshell.reload(false)
                running = false
            } else {
                // A second generation that drew its own menu: the old one has been torn down.
                Qt.exit(0)
            }
        }
    }
}
