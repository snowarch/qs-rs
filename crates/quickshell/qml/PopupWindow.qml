import QtQuick
import Quickshell

// An xdg_popup attached to another Quickshell window (a layer surface through LayerShellQt, or
// a toplevel). Shown only while its parent window is visible; placement goes to QtWayland's
// positioner, and a compositor dismissal sets `visible` back to false
// (ProxyWindowBase).
ProxyWindowBase {
    id: root
    visible: false
    property bool grabFocus: false
    readonly property PopupAnchor anchor: PopupAnchor {}
    property QtObject parentWindow
    property int relativeX
    property int relativeY
    onParentWindowChanged: { console.warn("PopupWindow.parentWindow is deprecated. Use PopupWindow.anchor.window."); anchor.window = parentWindow }
    onRelativeXChanged: anchor.rect.x = relativeX
    onRelativeYChanged: anchor.rect.y = relativeY

    readonly property QtObject _parent: anchor.window === root ? null : anchor.window
    _ready: _parent !== null && _parent._mapped
    screen: _parent ? _parent.screen : null

    readonly property var _placement: [anchor._windowRect, anchor.edges, anchor.gravity, anchor.adjustment]
    on_PlacementChanged: if (_window) { _place(); _reposition() }
    function _place() {
        const p = _placement
        QsWindowSupport.setPopupAnchor(_window, p[0], p[1], p[2], p[3])
    }
    // Placement is read when the popup surface is created: a change while shown re-creates it.
    property bool _placing: false
    function _reposition() {
        if (!_window || !_window.visible || _placing) return
        _placing = true
        _hide()
        _syncVisible()
        _placing = false
    }
    on_WindowChanged: if (_window) _place()
    on_AboutToShow: {
        _window.transientParent = _parent._backingWindow
        _window.flags = grabFocus ? Qt.Popup : Qt.ToolTip
        anchor.updateAnchor()
        _place()
    }
}
