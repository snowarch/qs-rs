import QtQuick
import Quickshell.Wayland

// Upstream's ScreencopyView. Screens (ShellScreen sources) are captured over wlr-screencopy by
// QsScreencopyCapture (screencopy.rs) and drawn by QsScreencopyItem (screencopy.h). Windows
// (Toplevel sources) are captured by QsWindowCapture over ext-image-copy-capture where the
// compositor offers it (imagecopy.rs), else through niri's ScreenCast (windowcast.rs), and drawn
// from DMA-BUFs by QsWindowCastItem (windowcast.h).
Item {
    id: root
    property QtObject captureSource: null
    property bool paintCursor: false
    property bool live: false
    // qs-rs only (hasVersion feature "niri-window-capture"): the niri window to record, exact where a
    // Toplevel is matched by app id and title. -1 records captureSource.
    property int niriWindowId: -1
    // qs-rs only (hasVersion feature "toplevel-image-copy"): the window's ext-foreign-toplevel
    // identifier (niri: its window id in decimal; Umbriel: its IPC id). Empty records captureSource.
    property string toplevelId: ""
    readonly property bool hasContent: _isWindow ? _window.hasContent : _capture.hasContent
    // The frame's size once the buffer transform is applied (a 90 or 270 degree screen swaps it).
    readonly property size sourceSize: !hasContent ? Qt.size(0, 0)
        : _isWindow ? _window.bufferSize
        : (_capture.bufferTransform & 1) ? Qt.size(_capture.bufferSize.height, _capture.bufferSize.width)
        : _capture.bufferSize
    // qs-rs only (hasVersion feature "screencopy-source-rect"): the part of the frame to show, in
    // pixels of a frame sourceRectBase big (the sourceSize it was picked on; empty = the current
    // one). Empty shows it all. On a resize each edge keeps its distance to the nearer window side.
    property rect sourceRect: Qt.rect(0, 0, 0, 0)
    property size sourceRectBase: Qt.size(0, 0)
    // sourceRect on the current frame: what is drawn and what the implicit size follows.
    readonly property rect effectiveSourceRect: _windowItem.mapCrop(sourceRect, sourceRectBase, sourceSize)
    // A nonzero side constrains the implicit size to it, keeping the shown part's aspect ratio.
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
        const c = constraintSize, s = Qt.size(effectiveSourceRect.width, effectiveSourceRect.height)
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
    readonly property bool _byId: niriWindowId >= 0 || toplevelId !== ""
    readonly property bool _isScreen: !_byId && _screenLike(captureSource)
    readonly property bool _isWindow: _byId || _toplevelLike(captureSource)
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
        sourceRect: root.sourceRect.width > 0 && root.sourceRect.height > 0 ? root.effectiveSourceRect : Qt.rect(0, 0, 0, 0)
    }

    QsWindowCapture {
        id: _window
        windowId: root.niriWindowId
        toplevelId: root.toplevelId
        appId: !root._byId && root._isWindow ? root.captureSource.appId : ""
        title: !root._byId && root._isWindow ? root.captureSource.title : ""
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
        sourceRect: root.sourceRect
        sourceRectBase: root.sourceRectBase
    }
}
