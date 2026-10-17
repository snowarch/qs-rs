import QtQuick
import Quickshell
import Quickshell.Wayland

// ScreencopyView exists with upstream's API (inir's OverviewWindow.qml:96 names it), so files
// that use it load; capture itself is still open.
ShellRoot {
    ScreencopyView { id: view; captureSource: null; live: true; paintCursor: true; constraintSize: Qt.size(200, 100) }
    Component.onCompleted: {
        const ok = view.live && view.paintCursor && !view.hasContent && view.sourceSize.width === 0 && view.constraintSize.width === 200
        if (!ok) console.log("FAILED")
        Qt.exit(ok ? 0 : 1)
    }
}
