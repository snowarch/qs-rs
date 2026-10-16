import QtQml

// Creates one delegate instance per model value, passing it as `modelData`. Instances whose
// value stays in the model are kept across model changes.
QtObject {
    id: root
    property string reloadableId
    default property Component delegate
    property var model: []
    readonly property list<QtObject> instances: _instances

    property list<QtObject> _instances
    property var _entries: []
    property bool _ready: false

    function _values() {
        const m = model
        if (m === null || m === undefined) return []
        if (Array.isArray(m)) return m
        if (typeof m.length === "number") { const out = []; for (let i = 0; i < m.length; i++) out.push(m[i]); return out }
        if (m.values !== undefined) return _valuesOf(m.values)
        console.warn("Non list data", m, "assigned to Variants.model, Ignoring.")
        return null
    }
    function _valuesOf(v) { const out = []; for (let i = 0; i < v.length; i++) out.push(v[i]); return out }

    function _update() {
        if (!_ready) return
        if (!delegate) { console.warn("Variants instance does not have a component specified"); return }
        const values = _values()
        if (values === null) return
        const kept = []
        for (const e of _entries) {
            if (values.some(v => v === e.value)) kept.push(e)
            else e.object.destroy()
        }
        const next = []
        for (const v of values) {
            if (next.some(e => e.value === v)) { console.warn("same value specified twice in Variants, duplicates will be ignored:", v); continue }
            let e = kept.find(k => k.value === v)
            if (!e) {
                const obj = delegate.createObject(root, { modelData: v })
                if (!obj) { console.warn("failed to create variant with object", v); continue }
                e = { value: v, object: obj }
            }
            next.push(e)
        }
        _entries = next
        _instances = next.map(e => e.object)
    }

    onModelChanged: _update()
    onDelegateChanged: { for (const e of _entries) e.object.destroy(); _entries = []; _update() }
    Component.onCompleted: { _ready = true; _update() }
}
