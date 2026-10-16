pragma Singleton
import QtQuick
import Quickshell
import Quickshell.Hyprland

// Hyprland state built from the IPC core: monitors, workspaces and toplevels refresh from
// `j/` requests whenever the event stream reports a change.
QtObject {
    id: root
    readonly property bool usingLua: false
    readonly property string requestSocketPath: HyprlandIpcCore.requestSocketPath
    readonly property string eventSocketPath: HyprlandIpcCore.eventSocketPath
    property QtObject focusedMonitor: null
    property QtObject focusedWorkspace: null
    property QtObject activeToplevel: null
    readonly property QtObject monitors: QtObject { property list<QtObject> values }
    readonly property QtObject workspaces: QtObject { property list<QtObject> values }
    readonly property QtObject toplevels: QtObject { property list<QtObject> values }

    signal rawEvent(var event)

    function dispatch(request) { _send("dispatch " + request, null) }
    function monitorFor(screen) {
        if (!screen) return null
        return monitors.values.find(m => m.name === screen.name) ?? null
    }
    function refreshMonitors() { _send("j/monitors", text => _sync(text, monitors, _monitorComponent, "name", _fillMonitor)) }
    function refreshWorkspaces() { _send("j/workspaces", text => _sync(text, workspaces, _workspaceComponent, "id", _fillWorkspace)) }
    function refreshToplevels() { _send("j/clients", text => _sync(text, toplevels, _toplevelComponent, "address", _fillToplevel)) }

    property int _nextId: 1
    property var _pending: ({})
    function _send(request, onReply) {
        if (!HyprlandIpcCore.available) return
        const id = _nextId++
        if (onReply) _pending[id] = onReply
        HyprlandIpcCore.request(id, request)
    }

    property Component _monitorComponent: Component { HyprlandMonitor {} }
    property Component _workspaceComponent: Component { HyprlandWorkspace {} }
    property Component _toplevelComponent: Component { HyprlandToplevel {} }

    // Keeps object identity per key across refreshes.
    function _sync(text, model, component, key, fill) {
        let list
        try { list = JSON.parse(text) } catch (e) { return }
        const old = {}
        for (const o of model.values) old[o["_key"]] = o
        const next = []
        for (const entry of list) {
            const k = String(entry[key])
            let o = old[k]
            if (!o) { o = component.createObject(root); o._key = k }
            delete old[k]
            fill(o, entry)
            next.push(o)
        }
        for (const k in old) old[k].destroy()
        model.values = next
        _relink()
    }
    function _fillMonitor(o, e) {
        o.id = e.id; o.name = e.name; o.description = e.description ?? ""; o.x = e.x; o.y = e.y
        o.width = e.width; o.height = e.height; o.scale = e.scale ?? 1; o.focused = !!e.focused; o.lastIpcObject = e
    }
    function _fillWorkspace(o, e) { o.id = e.id; o.name = e.name; o.hasFullscreen = !!e.hasfullscreen; o.lastIpcObject = e }
    function _fillToplevel(o, e) { o.address = String(e.address).replace(/^0x/, ""); o.title = e.title ?? ""; o.lastIpcObject = e }
    function _relink() {
        const byId = {}
        for (const w of workspaces.values) byId[w.id] = w
        for (const m of monitors.values) {
            m.activeWorkspace = byId[m.lastIpcObject.activeWorkspace?.id] ?? null
            if (m.focused) { focusedMonitor = m; focusedWorkspace = m.activeWorkspace }
        }
        for (const w of workspaces.values) {
            w.monitor = monitors.values.find(m => m.name === w.lastIpcObject.monitor) ?? null
            w.active = !!w.monitor && w.monitor.activeWorkspace === w
            w.focused = w === focusedWorkspace
        }
        for (const t of toplevels.values) {
            t.workspace = byId[t.lastIpcObject.workspace?.id] ?? null
            t.monitor = t.workspace ? t.workspace.monitor : null
        }
    }

    property Timer _refresh: Timer {
        interval: 16
        onTriggered: { root.refreshMonitors(); root.refreshWorkspaces(); root.refreshToplevels() }
    }

    property Connections _ipc: Connections {
        target: HyprlandIpcCore
        function onReply(id, text) {
            const cb = root._pending[id]
            delete root._pending[id]
            if (cb) cb(text)
        }
        function onRawEvent(name, data) {
            root.rawEvent({ name: name, data: data, parse: count => data.split(",", count) })
            if (name === "activewindowv2") {
                root.activeToplevel = root.toplevels.values.find(t => t.address === data) ?? null
                for (const t of root.toplevels.values) t.activated = t === root.activeToplevel
            }
            root._refresh.restart()
        }
    }

    Component.onCompleted: if (HyprlandIpcCore.available) _refresh.start()
}
