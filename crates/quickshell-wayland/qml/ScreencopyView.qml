import QtQuick
import Quickshell.Wayland

// Upstream's ScreencopyView. Screens (ShellScreen sources) are captured over wlr-screencopy by
// QsScreencopyCapture (screencopy.rs) and drawn by QsScreencopyItem (screencopy.h). Windows
// (Toplevel sources) need hyprland-toplevel-export or ext-image-copy-capture, which niri 26.04
// lacks: like upstream there, they are not captureable.
Item {
    id: root
    property QtObject captureSource: null
    property bool paintCursor: false
    property bool live: false
    readonly property bool hasContent: _capture.hasContent
    // The frame's size once the buffer transform is applied (a 90 or 270 degree screen swaps it).
    readonly property size sourceSize: !hasContent ? Qt.size(0, 0)
        : (_capture.bufferTransform & 1) ? Qt.size(_capture.bufferSize.height, _capture.bufferSize.width)
        : _capture.bufferSize
    // A nonzero side constrains the implicit size to it, keeping sourceSize's aspect ratio.
    property size constraintSize: Qt.size(0, 0)
    signal stopped()

    function captureFrame() {
        if (!_capture.captureFrame()) console.warn(root, "Cannot capture frame, as no recording context is ready.")
    }

    implicitWidth: _implicit.width
    implicitHeight: _implicit.height
    // Upstream divides by the constraint for a single-sided one (height / width); this keeps the
    // aspect ratio, which is what the property is documented to do.
    readonly property size _implicit: {
        const c = constraintSize, s = sourceSize
        if (c.width !== 0 && c.height !== 0) {
            if (s.width <= 0 || s.height <= 0) return Qt.size(0, 0)
            const k = Math.min(c.width / s.width, c.height / s.height)
            return Qt.size(s.width * k, s.height * k)
        }
        if (c.width !== 0) return Qt.size(c.width, s.width > 0 ? s.height * c.width / s.width : 0)
        if (c.height !== 0) return Qt.size(s.height > 0 ? s.width * c.height / s.height : 0, c.height)
        return s
    }

    readonly property bool _isScreen: captureSource !== null && captureSource._info !== undefined
    onCaptureSourceChanged: if (captureSource !== null && !_isScreen) console.warn(root, "Capture source set to non captureable object.")

    QsScreencopyCapture {
        id: _capture
        output: root._isScreen ? root.captureSource.name : ""
        paintCursor: root.paintCursor
        live: root.live
        onStopped: root.stopped()
    }

    QsScreencopyItem {
        anchors.fill: parent
        visible: root.hasContent
        image: _capture.frame
        bufferTransform: _capture.bufferTransform
    }
}
