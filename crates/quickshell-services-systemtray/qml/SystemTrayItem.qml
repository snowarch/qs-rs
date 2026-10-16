import QtQml
import Quickshell
import Quickshell.DBusMenu
import Quickshell.Services.SystemTray

// One StatusNotifierItem; `_key` is the watcher's "service/path".
QtObject {
    id: item
    property string id
    property string title
    property int status: Status.Active
    property int category: Category.ApplicationStatus
    readonly property string icon: {
        if (status === Status.NeedsAttention) {
            if (_attentionIconName) return Quickshell.iconPath(_attentionIconName + (_iconThemePath ? "?path=" + _iconThemePath : ""))
            return _attentionIconPixmap || _iconPixmap
        }
        if (_iconName && !_overlayIconName) return Quickshell.iconPath(_iconName + (_iconThemePath ? "?path=" + _iconThemePath : ""))
        return _iconPixmap
    }
    property string tooltipTitle
    property string tooltipDescription
    readonly property bool hasMenu: _menuPath !== "" && _menuPath !== "/NO_DBUSMENU"
    readonly property QsMenuHandle menu: hasMenu ? _menuHandle : null
    property bool onlyMenu

    function activate() { SystemTrayCore.activate(_key) }
    function secondaryActivate() { SystemTrayCore.secondaryActivate(_key) }
    function scroll(delta, horizontal) { SystemTrayCore.scroll(_key, delta, horizontal) }
    function display(parentWindow, relativeX, relativeY) {
        if (!hasMenu) return console.error("No menu present for", item)
    }

    property string _key
    property bool _ready: false
    property string _iconName
    property string _iconThemePath
    property string _attentionIconName
    property string _overlayIconName
    property string _iconPixmap
    property string _attentionIconPixmap
    property string _menuPath
    property DBusMenuHandle _menuHandle: DBusMenuHandle {}

    function _update(d) {
        id = d.id
        title = d.title
        status = Status._fromWire(d.status)
        category = Category._fromWire(d.category)
        _iconName = d.iconName
        _iconThemePath = d.iconThemePath
        _attentionIconName = d.attentionIconName
        _overlayIconName = d.overlayIconName
        _iconPixmap = d.iconPixmap
        _attentionIconPixmap = d.attentionIconPixmap
        tooltipTitle = d.tooltipTitle
        tooltipDescription = d.tooltipDescription
        _menuPath = d.menu
        // _key is "service/path"; the menu lives on the item's service.
        _menuHandle._setAddress(_key.slice(0, _key.indexOf("/")), hasMenu ? _menuPath : "")
        onlyMenu = d.onlyMenu
    }
}
