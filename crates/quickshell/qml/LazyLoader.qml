import QtQml
import Quickshell

// Creates its component on demand. `loading` incubates asynchronously, `active` forces creation
// to finish (blocking), `activeAsync` activates through an async load. `item` is owned here.
QtObject {
    id: root
    property string reloadableId
    default property Component component
    // Relative paths resolve against the file that instantiated this loader.
    property url source

    property bool loading: false
    property bool active: false
    property bool activeAsync: false
    readonly property QtObject item: _item

    property QtObject _item: null
    property var _incubator: null
    property bool _ready: false
    property Component _sourceComponent: null

    function _component() {
        if (component) return component
        if (source.toString() === "") return null
        const url = QsWindowSupport.resolvedUrl(root, source)
        if (!_sourceComponent || _sourceComponent.url.toString() !== url.toString())
            _sourceComponent = Qt.createComponent(url, Component.Asynchronous)
        return _sourceComponent
    }

    function _incubate() {
        if (!_ready || _item || _incubator) return
        const c = _component()
        if (!c) return
        if (c.status === Component.Loading) {
            c.statusChanged.connect(_incubate)
            return
        }
        if (c.status === Component.Error) {
            console.warn("LazyLoader:", c.errorString())
            return
        }
        const inc = c.incubateObject(null, {}, Qt.Asynchronous)
        if (!inc) return
        _incubator = inc
        if (inc.status === Component.Ready) {
            _finish()
        } else {
            inc.onStatusChanged = status => {
                if (status === Component.Ready) _finish()
                else if (status === Component.Error) { _incubator = null; loading = false }
            }
        }
    }

    function _finish() {
        const obj = _incubator.object
        _incubator = null
        _item = obj
        loading = false
        active = true
    }

    function _destroy() {
        if (_incubator) { _incubator.forceCompletion(); const o = _incubator.object; _incubator = null; if (o) o.destroy() }
        if (_item) { const o = _item; _item = null; o.destroy() }
    }

    onLoadingChanged: {
        if (loading && !_item) _incubate()
        else if (!loading && !active) _destroy()
    }
    onActiveChanged: {
        if (active) {
            if (_item) return
            _incubate()
            if (_incubator) { _incubator.forceCompletion(); if (_incubator) _finish() }
        } else {
            activeAsync = false
            _destroy()
        }
    }
    onActiveAsyncChanged: {
        if (activeAsync) { if (!_item) loading = true }
        else active = false
    }
    function _reload() { if (active || loading) { _destroy(); if (active) { active = false; active = true } else _incubate() } }
    onComponentChanged: _reload()
    onSourceChanged: _reload()

    Component.onCompleted: {
        _ready = true
        if (active) { active = false; active = true }
        else if (loading || activeAsync) _incubate()
    }
    Component.onDestruction: _destroy()
}
