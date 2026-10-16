import QtQuick
import Quickshell

// Where a PopupWindow attaches: a rect of a Quickshell window (or of an item inside one),
// minus margins, plus the anchor edges, gravity and adjustment of xdg_positioner.
QtObject {
    id: anchor
    property QtObject window: null
    property Item item: null
    component Box: QtObject {
        property int x
        property int y
        property int width
        property int height
    }
    component Margins: QtObject {
        property int left
        property int right
        property int top
        property int bottom
    }
    readonly property Box rect: Box {}
    readonly property Margins margins: Margins {}
    property int edges: Edges.Top | Edges.Left
    property int gravity: Edges.Bottom | Edges.Right
    property int adjustment: PopupAdjustment.Slide

    signal anchoring()

    // The window the popup is parented to: the item's window when an item is set.
    readonly property QtObject _proxy: item ? _itemProxy : window
    property QtObject _itemProxy: null
    property Connections _itemWindow: Connections {
        target: anchor.item
        function onWindowChanged() { anchor._itemProxy = QsWindowSupport.proxyOf(anchor.item) }
    }
    onItemChanged: { if (item) window = null; _itemProxy = QsWindowSupport.proxyOf(item) }
    // Windows register themselves when they complete, which can be after this anchor's item
    // was assigned.
    Component.onCompleted: Qt.callLater(() => { if (anchor.item) anchor._itemProxy = QsWindowSupport.proxyOf(anchor.item) })

    // Anchor rect in the parent window's coordinates.
    property rect _windowRect: Qt.rect(0, 0, 1, 1)
    readonly property rect _userRect: Qt.rect(rect.x, rect.y, rect.width, rect.height)
    on_UserRectChanged: updateAnchor()
    readonly property var _marginsKey: [margins.left, margins.right, margins.top, margins.bottom]
    on_MarginsKeyChanged: updateAnchor()

    function updateAnchor() {
        let r = _userRect
        if (item) {
            if (r.width <= 0 && r.height <= 0 && r.x === 0 && r.y === 0) r = Qt.rect(0, 0, item.width, item.height)
            const p = _proxy
            if (p) r = item.mapToItem(p.contentItem, r.x + margins.left, r.y + margins.top,
                r.width - margins.left - margins.right, r.height - margins.top - margins.bottom)
        } else {
            r = Qt.rect(r.x + margins.left, r.y + margins.top, r.width - margins.left - margins.right, r.height - margins.top - margins.bottom)
        }
        _windowRect = Qt.rect(Math.round(r.x), Math.round(r.y), Math.max(1, Math.round(r.width)), Math.max(1, Math.round(r.height)))
    }
}
