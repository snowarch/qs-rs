import QtQuick
import Quickshell
import Quickshell.Wayland

// A wlr-layer-shell surface (through LayerShellQt). Layer, namespace and keyboard focus come
// from the WlrLayershell attached object, as in Quickshell.
ProxyWindowBase {
    id: root
    property PanelAnchors anchors: PanelAnchors {}
    property PanelMargins margins: PanelMargins {}
    readonly property QsPanelExclusion _exclusion: QsPanelExclusion {}
    property alias exclusiveZone: root._exclusion.exclusiveZone
    property alias exclusionMode: root._exclusion.exclusionMode
    property bool aboveWindows: true
    property bool focusable: false
    color: "white"

    onAboveWindowsChanged: WlrLayershell.layer = aboveWindows ? WlrLayer.Top : WlrLayer.Bottom
    onFocusableChanged: WlrLayershell.keyboardFocus = focusable ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None

    readonly property int _layer: WlrLayershell.layer
    readonly property string _namespace: WlrLayershell.namespace
    readonly property int _keyboardFocus: WlrLayershell.keyboardFocus

    // Edge the auto exclusive zone applies to: one anchored edge, optionally stretched along
    // both perpendicular edges.
    readonly property int _exclusionEdge: {
        const a = anchors
        const h = a.left === a.right, v = a.top === a.bottom
        if (a.top && !a.bottom && h) return 1
        if (a.bottom && !a.top && h) return 2
        if (a.left && !a.right && v) return 4
        if (a.right && !a.left && v) return 8
        return 0
    }
    readonly property int _exclusiveZone: {
        if (exclusionMode === ExclusionMode.Ignore) return -1
        if (exclusionMode === ExclusionMode.Normal) return exclusiveZone
        switch (_exclusionEdge) {
        case 1: return implicitHeight + margins.bottom
        case 2: return implicitHeight + margins.top
        case 4: return implicitWidth + margins.right
        case 8: return implicitWidth + margins.left
        }
        return 0
    }

    readonly property int _anchorBits: (anchors.top ? 1 : 0) | (anchors.bottom ? 2 : 0) | (anchors.left ? 4 : 0) | (anchors.right ? 8 : 0)
    // Any input change reconfigures the surface; LayerShellQt applies live what the protocol allows.
    readonly property var _config: [_anchorBits, _layer, _keyboardFocus, _exclusiveZone, margins.left, margins.top, margins.right, margins.bottom, _namespace]
    on_ConfigChanged: _configure()
    function _configure() {
        const c = _config
        QsLayerShell.configure(_window, c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8])
    }
    // The surface role is fixed when the platform window is created, so configure first.
    property bool _configured: false
    _ready: _configured
    Component.onCompleted: { _configure(); _configured = true }

    // Layer, namespace and output are fixed when the surface is created: recreate on change.
    function _recreate() {
        if (!_window.visible) return
        _hide()
        _syncVisible()
    }
    on_LayerChanged: _recreate()
    on_NamespaceChanged: _recreate()
    onScreenChanged: _recreate()
}
