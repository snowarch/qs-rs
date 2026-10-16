pragma Singleton
import QtQml
import Quickshell.Services.Pipewire

// PipeWire graph built from PwCore's updates: one object per node, link and link group, kept
// by id so references stay valid. Model changes are flushed once per event loop turn.
QtObject {
    id: root
    readonly property QtObject nodes: QtObject { property list<PwNode> values }
    readonly property QtObject links: QtObject { property list<PwLink> values }
    readonly property QtObject linkGroups: QtObject { property list<PwLinkGroup> values }
    readonly property bool ready: PwCore.ready

    property PwNode defaultAudioSink: null
    property PwNode defaultAudioSource: null
    property PwNode preferredDefaultAudioSink: null
    property PwNode preferredDefaultAudioSource: null

    property var _nodes: ({})
    property var _links: ({})
    property var _groups: ({})
    property var _defaults: ({})
    property bool _syncing: false
    property bool _dirty: false

    property Component _nodeComponent: Component { PwNode {} }
    property Component _audioComponent: Component { PwNodeAudio {} }
    property Component _linkComponent: Component { PwLink {} }
    property Component _groupComponent: Component { PwLinkGroup {} }

    onPreferredDefaultAudioSinkChanged: _setPreferred("default.configured.audio.sink", preferredDefaultAudioSink)
    onPreferredDefaultAudioSourceChanged: _setPreferred("default.configured.audio.source", preferredDefaultAudioSource)

    function _setPreferred(key, node) {
        if (_syncing) return
        if (node) PwCore.setDefault(key, node.name)
    }

    function _schedule() {
        if (_dirty) return
        _dirty = true
        Qt.callLater(_flush)
    }

    function _byName(name) {
        if (!name) return null
        for (const k in _nodes) if (_nodes[k].name === name) return _nodes[k]
        return null
    }

    function _flush() {
        _dirty = false
        nodes.values = Object.values(_nodes)
        links.values = Object.values(_links)
        linkGroups.values = Object.values(_groups)
        _syncing = true
        const resolve = key => _byName(_defaults[key])
        const sink = resolve("default.audio.sink"), source = resolve("default.audio.source")
        const psink = resolve("default.configured.audio.sink"), psource = resolve("default.configured.audio.source")
        if (defaultAudioSink !== sink) defaultAudioSink = sink
        if (defaultAudioSource !== source) defaultAudioSource = source
        if (preferredDefaultAudioSink !== psink) preferredDefaultAudioSink = psink
        if (preferredDefaultAudioSource !== psource) preferredDefaultAudioSource = psource
        _syncing = false
    }

    function _node(id) {
        let n = _nodes[id]
        if (!n) {
            n = _nodeComponent.createObject(root, { id: id })
            _nodes[id] = n
        }
        return n
    }

    function _relinkGroup(key) {
        const g = _groups[key]
        if (!g) return
        g.source = _nodes[g._outputNode] ?? null
        g.target = _nodes[g._inputNode] ?? null
    }

    property Connections _core: Connections {
        target: PwCore
        function onNodeChanged(id, props) {
            const n = root._node(id)
            n.properties = props
            // Like quickshell, keys missing from an update keep their previous value.
            if (props["node.name"] !== undefined) n.name = props["node.name"]
            if (props["node.description"] !== undefined) n.description = props["node.description"]
            if (props["node.nick"] !== undefined) n.nickname = props["node.nick"]
            const category = props["media.category"]
            if (category === "Monitor" || category === "Manager") n._isMonitor = true
            if (props["media.class"] !== undefined) n.type = PwNodeType._fromMediaClass(props["media.class"])
            if ((n.type & PwNodeType.Audio) && !n.audio) n.audio = root._audioComponent.createObject(n, { _node: id })
            if (!n.audio) n.ready = true
            for (const l of Object.values(root._links)) {
                if (l._out === id) l.source = n
                if (l._in === id) l.target = n
            }
            for (const k in root._groups) root._relinkGroup(k)
            root._schedule()
        }
        function onNodeAudioChanged(id, volumes, channels, muted) {
            const n = root._nodes[id]
            if (!n) return
            if (!n.audio) n.audio = root._audioComponent.createObject(n, { _node: id })
            n.audio._update(Array.from(volumes), Array.from(channels), muted)
            n.ready = true
        }
        function onNodeRemoved(id) {
            const n = root._nodes[id]
            if (!n) return
            delete root._nodes[id]
            for (const k in root._groups) root._relinkGroup(k)
            root._flush()
            n.destroy()
        }
        function onLinkChanged(id, out, input, state) {
            let l = root._links[id]
            if (!l) {
                l = root._linkComponent.createObject(root, { id: id })
                l._out = out
                l._in = input
                root._links[id] = l
            }
            l.source = root._nodes[out] ?? null
            l.target = root._nodes[input] ?? null
            l.state = state
            const key = out + ":" + input
            let g = root._groups[key]
            if (!g) {
                g = root._groupComponent.createObject(root, { _outputNode: out, _inputNode: input })
                g._links = [id]
                root._groups[key] = g
                root._relinkGroup(key)
            } else if (!g._links.includes(id)) {
                g._links.push(id)
            }
            if (g._links[0] === id) g.state = state
            root._schedule()
        }
        function onLinkRemoved(id) {
            const l = root._links[id]
            if (!l) return
            delete root._links[id]
            const key = l._out + ":" + l._in
            const g = root._groups[key]
            let deadGroup = null
            if (g) {
                g._links = g._links.filter(x => x !== id)
                if (g._links.length === 0) {
                    delete root._groups[key]
                    deadGroup = g
                } else {
                    g.state = root._links[g._links[0]]?.state ?? PwLinkState.Unlinked
                }
            }
            root._flush()
            l.destroy()
            if (deadGroup) deadGroup.destroy()
        }
        function onDefaultChanged(key, name) {
            root._defaults[key] = name
            root._schedule()
        }
    }
}
