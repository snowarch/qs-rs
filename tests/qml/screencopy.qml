import QtQuick
import Quickshell
import Quickshell.Wayland

// ScreencopyView exists with upstream's API (inir's OverviewWindow.qml:96 names it) plus qs-rs's
// niriWindowId, and says so through hasVersion. Capture itself needs a compositor: checked in
// nested runs against niri's ScreenCast, not here.
ShellRoot {
    ScreencopyView { id: view; captureSource: null; live: true; paintCursor: true; constraintSize: Qt.size(200, 100) }
    Component.onCompleted: {
        const ok = view.live && view.paintCursor && !view.hasContent && view.sourceSize.width === 0 && view.constraintSize.width === 200
            && view.niriWindowId === -1 && Quickshell.hasVersion(0, 3, ["toplevel-capture", "niri-window-capture"])
        if (!ok) console.log("FAILED")
        Qt.exit(ok ? 0 : 1)
    }
}
