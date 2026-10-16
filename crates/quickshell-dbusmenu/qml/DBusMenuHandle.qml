import QtQml
import Quickshell
import Quickshell.DBusMenu

// Upstream DBusMenuHandle: a menu at (service, path). While referenced (QsMenuOpener) it keeps a
// DBusMenuClient and the whole item tree; `menu` is the root item once its layout arrived.
QsMenuHandle {
    id: root
    property string _service
    property string _path
    property int _refs: 0
    property QtObject _client: null
    property var _items: ({})
    property string _iconThemePath
    property QtObject _root: null

    function _setAddress(service, path) {
        if (service === _service && path === _path) return
        _service = service
        _path = path
        _rebuild()
    }
    function refHandle() {
        _refs++
        if (_refs === 1 || !_client) _rebuild()
        // Refresh in case a client does not send updates and another reference is open.
        else _client.updateLayout(0)
    }
    function unrefHandle() {
        _refs--
        if (_refs === 0) _rebuild()
    }

    property Component _clientComponent: Component {
        // Events of a client that was already replaced are dropped.
        DBusMenuClient {
            id: client
            onLayout: json => { if (root._client === client) root._applyLayout(JSON.parse(json)) }
            onPropertiesUpdated: json => { if (root._client === client) root._applyProperties(JSON.parse(json)) }
            onIconThemePath: path => { if (root._client === client) root._iconThemePath = path }
        }
    }
    property Component _itemComponent: Component { DBusMenuItem {} }

    function _rebuild() {
        if (_client) {
            _client.destroy()
            _client = null
            menu = null
            for (const id in _items) _items[id].destroy()
            _items = {}
            _root = null
        }
        if (_refs > 0 && _service !== "" && _path !== "") {
            _client = _clientComponent.createObject(root)
            _root = _item(0)
            _client._qsConnect(_service, _path)
            _client.aboutToShow(0)
        }
    }

    function _item(id) {
        let it = _items[id]
        if (!it) {
            it = _itemComponent.createObject(root, { _id: id, _handle: root })
            _items[id] = it
        }
        return it
    }
    function _parentOf(item) {
        for (const id in _items) if (_items[id]._childIds.includes(item._id)) return _items[id]
        return null
    }
    function _removeRecursive(id) {
        const it = _items[id]
        if (!it) return
        for (const c of it._childIds) _removeRecursive(c)
        delete _items[id]
        it.destroy()
    }
    function _refreshChildren(item) {
        if (!item) return
        item.children.values = item._childIds.map(c => _items[c]).filter(c => c && c._visible)
    }

    function _applyNode(node) {
        const item = _items[node.id]
        if (!item) return
        item._update(node.props, true)
        const ids = node.children.map(c => c.id)
        for (const old of item._childIds) if (!ids.includes(old)) _removeRecursive(old)
        for (const c of node.children) _item(c.id)
        item._childIds = ids
        for (const c of node.children) _applyNode(c)
        _refreshChildren(item)
    }
    function _applyLayout(node) {
        _applyNode(node)
        if (node.id === 0 && !menu) menu = _root
    }
    function _applyProperties(u) {
        for (const [id, props] of u.updated) _items[id]?._update(props, false)
        for (const [id, names] of u.removed) _items[id]?._update({}, false, names)
    }

    Component.onDestruction: { _refs = 0; _rebuild() }
}
