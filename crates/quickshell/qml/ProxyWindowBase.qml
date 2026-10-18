import QtQuick
import Quickshell

// Shared base of Quickshell windows: a QtObject that puts its children in its own content item
// and exposes Quickshell's window properties. Like upstream, the backing QQuickWindow comes once
// the proxy is complete (QsWindowSupport.acquireWindow): a new one, or the same declaration's
// window from before a reload, so reloading never unmaps it.
QtObject {
    id: root
    default property alias data: _content.data
    readonly property Item contentItem: _content
    property bool visible: true
    property int implicitWidth: 100
    property int implicitHeight: 100
    property int width: _window ? _window.width : implicitWidth
    property int height: _window ? _window.height : implicitHeight
    readonly property real devicePixelRatio: _window ? _window.devicePixelRatio : 1
    property QtObject screen: null
    property color color: "white"
    property Region mask: null
    readonly property QtObject windowTransform: null
    readonly property bool backingWindowVisible: _window ? _window.visible : false
    // True once the shown window drew its first frame, i.e. its surface exists on the
    // compositor side (popups can only attach to a mapped parent).
    property bool _mapped: false
    property var surfaceFormat: ({})
    property bool updatesEnabled: true
    // A plain QQuickWindow (C++), so not typed as QtQuick.Window (QQuickWindowQmlImpl).
    property QtObject _window: null
    readonly property QtObject _backingWindow: _window
    // What a window must share with the new declaration to be reused across a reload; empty
    // never reuses (popups).
    property string _reuseTag: ""

    // Extra conditions subtypes add before the window may be shown.
    property bool _ready: true

    onWidthChanged: if (width !== (_window ? _window.width : implicitWidth)) implicitWidth = width
    onHeightChanged: if (height !== (_window ? _window.height : implicitHeight)) implicitHeight = height

    function _applyMask() { if (_window) QsWindowSupport.applyMask(_window, root.mask) }
    onMaskChanged: _applyMask()

    property Connections _maskWatcher: Connections {
        target: root.mask
        ignoreUnknownSignals: true
        function onChanged() { root._applyMask() }
    }

    property Item _content: Item {
        id: _content
        width: root._window ? root._window.width : root.implicitWidth
        height: root._window ? root._window.height : root.implicitHeight
    }

    // Bindings and handlers on the window live in the proxy, so they go with it when a reload
    // hands the window to the next generation. RestoreNone: a frozen window must not resize.
    property list<QtObject> _windowBindings: [
        Binding { target: root._window; property: "width"; value: root.implicitWidth; restoreMode: Binding.RestoreNone },
        Binding { target: root._window; property: "height"; value: root.implicitHeight; restoreMode: Binding.RestoreNone },
        Binding { target: root._window; property: "color"; value: root.color; restoreMode: Binding.RestoreNone }
    ]
    function _applyScreen() {
        QsWindowSupport.setWindowScreen(_window, root.screen ? root.screen._info : Qt.application.screens[0])
    }
    onScreenChanged: _applyScreen()
    property Connections _windowEvents: Connections {
        target: root._window
        function onFrameSwapped() { if (!root._mapped) root._mapped = true }
        // Closed from outside: like upstream, `visible` turns false before `closed`.
        function onVisibleChanged() {
            if (root._window.visible) return
            root._mapped = false
            if (!root._hiding) { root.visible = false; root.closed() }
        }
        function onWidthChanged() { root._applyMask() }
        function onHeightChanged() { root._applyMask() }
    }

    // Emitted right before the backing window is shown, for subtypes that configure the
    // surface at creation.
    signal _aboutToShow()
    // The window was closed from outside (compositor dismissal, close request), not by us.
    signal closed()
    property bool _hiding: false
    function _hide() {
        _hiding = true
        _window.visible = false
        _hiding = false
    }
    function _syncVisible() {
        if (!_window) return
        const show = root.visible && root._ready && _completed
        // A window taken over from before a reload is still frozen on its old frame: it either
        // thaws when shown, or is hidden if it still is not wanted a while later.
        const frozen = QsWindowSupport.isFrozen(_window)
        if (show) {
            _thawLater.stop()
            if (frozen) QsWindowSupport.thawWindow(_window)
            // The output is fixed when the surface is created: set it right before.
            else if (!_window.visible) { _applyScreen(); _aboutToShow() }
            _window.visible = true
        } else if (_window.visible) {
            if (frozen) _thawLater.restart()
            else _hide()
        }
    }
    property Timer _thawLater: Timer {
        interval: 3000
        onTriggered: if (root._window) { QsWindowSupport.thawWindow(root._window); root._syncVisible() }
    }
    property bool _completed: false
    onVisibleChanged: _syncVisible()
    on_ReadyChanged: _syncVisible()
    Component.onCompleted: {
        _window = QsWindowSupport.acquireWindow(root, _content, _reuseTag, root.screen ? root.screen.name : "")
        _applyScreen()
        _completed = true
        _applyMask()
        _syncVisible()
    }
}
