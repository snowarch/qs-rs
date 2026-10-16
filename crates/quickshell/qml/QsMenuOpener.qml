import QtQml
import Quickshell

// Upstream QsMenuOpener: shows `menu` (a QsMenuHandle) while set. It references the handle (so a
// DBusMenu loads) and the loaded entry (sending `opened`/`closed`); `children` are the entry's.
QtObject {
    id: root
    property QsMenuHandle menu: null
    readonly property ObjectModel children: menu?.menu?.children ?? _empty

    property ObjectModel _empty: ObjectModel {}
    property QsMenuHandle _handle: null
    property QtObject _entry: null

    function _setEntry(entry) {
        if (entry === _entry) return
        if (_entry) _entry.unref()
        _entry = entry
        if (_entry) _entry.ref()
    }
    function _onHandleMenu() { _setEntry(_handle?.menu ?? null) }
    function _release() {
        _setEntry(null)
        if (_handle) {
            _handle.menuChanged.disconnect(_onHandleMenu)
            _handle.unrefHandle()
            _handle = null
        }
    }

    onMenuChanged: {
        _release()
        if (!menu) return
        _handle = menu
        _handle.menuChanged.connect(_onHandleMenu)
        _onHandleMenu()
        _handle.refHandle()
    }
    Component.onCompleted: if (menu && !_handle) menuChanged()
    Component.onDestruction: _release()
}
