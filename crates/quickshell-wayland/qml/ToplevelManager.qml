pragma Singleton
import QtQml
import Quickshell
import Quickshell.Wayland

// Toplevels in the order they became ready; one Toplevel per handle, removed when it closes.
QtObject {
    id: root
    readonly property QtObject toplevels: ObjectModel {}
    readonly property Toplevel activeToplevel: _active

    property Toplevel _active: null
    property var _byKey: ({})
    property Component _component: Component { Toplevel {} }

    property Connections _core: Connections {
        target: ToplevelCore
        function onToplevelUpdated(key, appId, title, parent, activated, maximized, minimized, fullscreen, screens) {
            let t = root._byKey[key]
            const isNew = !t
            if (isNew) {
                t = root._component.createObject(root, { _qsKey: key })
                root._byKey[key] = t
            }
            t._appId = appId
            t._title = title
            t._parent = root._byKey[parent] ?? null
            t._screens = Quickshell.screens.filter(s => screens.includes(s.name))
            t._qsSetState(maximized, minimized, fullscreen)
            t._activated = activated
            if (activated) root._active = t
            else if (root._active === t) root._active = null
            if (isNew) root.toplevels.values = [...root.toplevels.values, t]
        }
        function onToplevelClosed(key) {
            const t = root._byKey[key]
            if (!t) return
            delete root._byKey[key]
            t.closed()
            if (root._active === t) root._active = null
            root.toplevels.values = root.toplevels.values.filter(x => x !== t)
            Quickshell._retire(t)
        }
    }
}
