import QtQuick
import Quickshell.Wayland

// Upstream's ScreencopyView. Screens (ShellScreen sources) are captured over wlr-screencopy by
// QsScreencopyCapture (screencopy.rs) and drawn by QsScreencopyItem (screencopy.h). Windows
// (Toplevel sources) are recorded through niri's ScreenCast by QsWindowCapture (windowcast.rs)
// and drawn from DMA-BUFs by QsWindowCastItem (windowcast.h); niri 26.04 has none of the
// Wayland window capture protocols upstream uses.
Item {
    id: root
    property QtObject captureSource: null
    property bool paintCursor: false
    property bool live: false
    // qs-rs only (hasVersion feature "toplevel-capture"): the niri window to record, exact where a
    // Toplevel is matched by app id and title. -1 records captureSource.
    property int niriWindowId: -1
    readonly property bool hasContent: _isWindow ? _window.hasContent : _capture.hasContent
    // The frame's size once the buffer transform is applied (a 90 or 270 degree screen swaps it).
    readonly property size sourceSize: !hasContent ? Qt.size(0, 0)
        : _isWindow ? _window.bufferSize
        : (_capture.bufferTransform & 1) ? Qt.size(_capture.bufferSize.height, _capture.bufferSize.width)
        : _capture.bufferSize
    // A nonzero side constrains the implicit size to it, keeping sourceSize's aspect ratio.
    property size constraintSize: Qt.size(0, 0)
    signal stopped()

    // qs-rs only (hasVersion feature "screencopy-luminance"): mean Rec. 709 luma of the current
    // frame, 0 to 1, or -1 without one.
    function meanLuminance(): real { return _isWindow ? -1 : _capture.meanLuminance() }

    function captureFrame() {
        if (!(_isWindow ? _window.captureFrame() : _capture.captureFrame())) console.warn(root, "Cannot capture frame, as no recording context is ready.")
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

    // A ShellScreen wraps Qt's screen info, a Toplevel has an app id; nothing else is captureable.
    function _screenLike(source: QtObject): bool { return source !== null && source._info !== undefined }
    function _toplevelLike(source: QtObject): bool { return source !== null && source.appId !== undefined && source._qsKey !== undefined }
    readonly property bool _isScreen: niriWindowId < 0 && _screenLike(captureSource)
    readonly property bool _isWindow: niriWindowId >= 0 || _toplevelLike(captureSource)
    // Not through the properties above: a change handler can run before the bindings they feed.
    onCaptureSourceChanged: if (captureSource !== null && !_screenLike(captureSource) && !_toplevelLike(captureSource)) console.warn(root, "Capture source set to non captureable object.")

    QsScreencopyCapture {
        id: _capture
        output: root._isScreen ? root.captureSource.name : ""
        paintCursor: root.paintCursor
        live: root.live
        onStopped: root.stopped()
    }

    QsScreencopyItem {
        anchors.fill: parent
        visible: !root._isWindow && root.hasContent
        image: _capture.frame
        bufferTransform: _capture.bufferTransform
    }

    QsWindowCapture {
        id: _window
        windowId: root.niriWindowId
        appId: root.niriWindowId < 0 && root._isWindow ? root.captureSource.appId : ""
        title: root.niriWindowId < 0 && root._isWindow ? root.captureSource.title : ""
        paintCursor: root.paintCursor
        live: root.live
        onFrameReady: _windowItem.update()
        onStopped: root.stopped()
    }

    QsWindowCastItem {
        id: _windowItem
        anchors.fill: parent
        visible: root._isWindow && root.hasContent
        frames: _window.frames
    }
}
