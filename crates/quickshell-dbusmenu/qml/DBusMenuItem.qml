import QtQml
import Quickshell

// One com.canonical.dbusmenu item (upstream DBusMenuItem). `_handle` owns the item map and the
// client; `children` holds the visible children in server order.
QsMenuEntry {
    id: item
    property int _id
    property QtObject _handle
    property var _childIds: []
    property bool _visible: true
    property bool _displayChildren: false
    property string _iconName
    property string _iconData

    hasChildren: _displayChildren || _id === 0
    icon: _iconName ? Quickshell.iconPath(_iconName + (_handle?._iconThemePath ? "?path=" + _handle._iconThemePath : ""))
                    : _iconData

    onOpened: _handle?._client?.sendEvent(_id, "opened")
    onClosed: _handle?._client?.sendEvent(_id, "closed")
    onTriggered: _handle?._client?.sendEvent(_id, "clicked")

    // `full` is a whole property set (GetLayout): absent keys reset to their default, like
    // upstream. A partial update (ItemsPropertiesUpdated) only touches what it names; `removed`
    // names reset.
    function _update(p, full, removed) {
        const has = k => p[k] !== undefined
        const reset = k => full || (removed ?? []).includes(k)
        if (has("label")) {
            // Mnemonic underscores are dropped from the clean label like upstream.
            let s = String(p.label), out = ""
            for (let i = 0; i < s.length; i++) {
                if (s[i] === "_" && i < s.length - 1) { i++; out += s[i]; continue }
                out += s[i]
            }
            text = out
        } else if (reset("label")) text = ""
        if (has("enabled")) enabled = !!p.enabled; else if (reset("enabled")) enabled = true
        if (has("visible")) _visible = !!p.visible; else if (reset("visible")) _visible = true
        if (has("icon-name")) _iconName = p["icon-name"]; else if (reset("icon-name")) _iconName = ""
        if (has("icon-data")) _iconData = p["icon-data"]; else if (reset("icon-data")) _iconData = ""
        if (has("type")) isSeparator = p.type === "separator"; else if (reset("type")) isSeparator = false
        if (has("toggle-type")) {
            const t = p["toggle-type"]
            buttonType = t === "checkmark" ? QsMenuButtonType.CheckBox : t === "radio" ? QsMenuButtonType.RadioButton : QsMenuButtonType.None
        } else if (reset("toggle-type")) buttonType = QsMenuButtonType.None
        if (has("toggle-state")) {
            const s = p["toggle-state"]
            checkState = s === 0 ? Qt.Unchecked : s === 1 ? Qt.Checked : Qt.PartiallyChecked
        } else if (reset("toggle-state")) checkState = Qt.Unchecked
        if (has("children-display")) _displayChildren = p["children-display"] === "submenu"
        else if (reset("children-display")) _displayChildren = false
    }

    on_VisibleChanged: _handle?._refreshChildren(_handle._parentOf(item))
}
