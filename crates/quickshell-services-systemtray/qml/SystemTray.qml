pragma Singleton
import QtQml
import Quickshell.Services.SystemTray

// Tray items by watcher key; an item joins `items` once its properties were read.
QtObject {
    id: root
    readonly property QtObject items: QtObject { property list<SystemTrayItem> values }

    property var _items: ({})
    property Component _component: Component { SystemTrayItem {} }

    property Connections _core: Connections {
        target: SystemTrayCore
        function onItemChanged(key, json) {
            let item = root._items[key]
            if (!item) { item = root._component.createObject(root, { _key: key }); root._items[key] = item }
            item._update(JSON.parse(json))
            if (!item._ready) { item._ready = true; root.items.values.push(item) }
        }
        function onItemRemoved(key) {
            const item = root._items[key]
            if (!item) return
            delete root._items[key]
            root.items.values = Array.from(root.items.values).filter(x => x !== item)
            item.destroy()
        }
    }
}
