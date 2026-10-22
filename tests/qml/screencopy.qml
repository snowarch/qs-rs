import QtQuick
import Quickshell
import Quickshell.Wayland

// ScreencopyView exists with upstream's API (inir's OverviewWindow.qml:96 names it) plus qs-rs's
// niriWindowId, and says so through hasVersion. Capture itself needs a compositor: checked in
// nested runs against niri's ScreenCast, not here.
ShellRoot {
    ScreencopyView { id: view; captureSource: null; live: true; paintCursor: true; constraintSize: Qt.size(200, 100) }
    QsWindowCastItem { id: cast }
    QsScreencopyItem { id: shot }
    function same(r, x, y, w, h): bool { return Math.abs(r.x - x) < 1e-6 && Math.abs(r.y - y) < 1e-6 && Math.abs(r.width - w) < 1e-6 && Math.abs(r.height - h) < 1e-6 }
    // A crop's edges keep their distance to the nearer side; empty means everything; it never leaves the frame.
    function cropOk(): bool {
        const map = (r, b, s) => cast.mapCrop(r, b, s)
        return same(map(Qt.rect(0, 0, 0, 0), Qt.size(0, 0), Qt.size(800, 600)), 0, 0, 800, 600)
            && same(map(Qt.rect(10, 20, 100, 50), Qt.size(0, 0), Qt.size(800, 600)), 10, 20, 100, 50)
            // A sidebar on the right, the window 200 wider: it moves with the right side, same size.
            && same(map(Qt.rect(600, 0, 200, 600), Qt.size(800, 600), Qt.size(1000, 600)), 800, 0, 200, 600)
            // A status line at the bottom spanning the middle: wider with the window, still at the bottom.
            && same(map(Qt.rect(0, 560, 800, 40), Qt.size(800, 600), Qt.size(1000, 700)), 0, 660, 1000, 40)
            // Shrunk past the crop: clamped, at least one pixel.
            && same(map(Qt.rect(700, 0, 100, 100), Qt.size(800, 600), Qt.size(50, 600)), 0, 0, 50, 100)
            && same(map(Qt.rect(10, 10, 5, 5), Qt.size(800, 600), Qt.size(0, 0)), 0, 0, 0, 0)
            && same(view.effectiveSourceRect, 0, 0, 0, 0)
            // Screens: a rect of the rotated view back in buffer pixels (buffer 200x100).
            && same(shot.bufferRect(Qt.rect(10, 20, 30, 40), Qt.size(200, 100), 0), 10, 20, 30, 40)
            && same(shot.bufferRect(Qt.rect(90, 0, 10, 10), Qt.size(200, 100), 1), 0, 0, 10, 10)
            && same(shot.bufferRect(Qt.rect(0, 0, 10, 10), Qt.size(200, 100), 2), 190, 90, 10, 10)
            && same(shot.bufferRect(Qt.rect(0, 0, 10, 10), Qt.size(200, 100), 4), 190, 0, 10, 10)
    }
    Component.onCompleted: {
        const ok = cropOk() && view.live && view.paintCursor && !view.hasContent && view.sourceSize.width === 0 && view.constraintSize.width === 200
            && view.niriWindowId === -1 && Quickshell.hasVersion(0, 3, ["toplevel-capture", "niri-window-capture", "screencopy-source-rect"])
        if (!ok) console.log("FAILED")
        Qt.exit(ok ? 0 : 1)
    }
}
