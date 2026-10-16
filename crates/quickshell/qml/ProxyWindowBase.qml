import QtQuick
import QtQuick.Window as QW
import Quickshell

// Shared base of Quickshell windows: a QtObject that owns a QQuickWindow, puts its children in
// the window's content item and exposes Quickshell's window properties.
QtObject {
    id: root
    default property alias data: _window.data
    readonly property Item contentItem: _window.contentItem
    property bool visible: true
    property int implicitWidth: 100
    property int implicitHeight: 100
    property int width: _window.width
    property int height: _window.height
    readonly property real devicePixelRatio: _window.devicePixelRatio
    property QtObject screen: null
    property color color: "white"
    property Region mask: null
    readonly property QtObject windowTransform: null
    readonly property bool backingWindowVisible: _window.visible
    // True once the shown window drew its first frame, i.e. its surface exists on the
    // compositor side (popups can only attach to a mapped parent).
    property bool _mapped: false
    property var surfaceFormat: ({})
    property bool updatesEnabled: true
    readonly property QW.Window _backingWindow: _window

    // Extra conditions subtypes add before the window may be shown.
    property bool _ready: true

    onWidthChanged: if (width !== _window.width) implicitWidth = width
    onHeightChanged: if (height !== _window.height) implicitHeight = height

    function _applyMask() { QsWindowSupport.applyMask(_window, root.mask) }
    onMaskChanged: _applyMask()

    property Connections _maskWatcher: Connections {
        target: root.mask
        ignoreUnknownSignals: true
        function onChanged() { root._applyMask() }
    }

    property QW.Window _window: QW.Window {
        id: _window
        onFrameSwapped: if (!root._mapped) root._mapped = true
        onVisibleChanged: if (!visible) root._mapped = false
        width: root.implicitWidth
        height: root.implicitHeight
        color: root.color
        screen: root.screen ? root.screen._info : Qt.application.screens[0]
        visible: false
        onWidthChanged: root._applyMask()
        onHeightChanged: root._applyMask()
    }

    // Emitted right before the backing window is shown, for subtypes that configure the
    // surface at creation.
    signal _aboutToShow()
    function _syncVisible() {
        const show = root.visible && root._ready && _completed
        if (show && !_window.visible) _aboutToShow()
        _window.visible = show
    }
    property bool _completed: false
    onVisibleChanged: _syncVisible()
    on_ReadyChanged: _syncVisible()
    Component.onCompleted: {
        QsWindowSupport.registerProxy(_window, root)
        _completed = true
        _applyMask()
        _syncVisible()
    }
}
