import QtQml
import Quickshell.Services.Pipewire

// Audio state of a node. Values come from the node's Props; writing volume, volumes or muted
// sends a command and the confirmed value comes back from PipeWire.
QtObject {
    id: root
    property int _node: -1
    property bool _syncing: false

    property bool muted: false
    property real volume: 0
    property list<real> volumes
    property list<int> channels

    function _average(list) {
        if (list.length === 0) return 0
        let total = 0
        for (const v of list) total += v
        return total / list.length
    }

    function _update(newVolumes, newChannels, newMuted) {
        _syncing = true
        if (String(channels) !== String(newChannels)) channels = newChannels
        if (String(volumes) !== String(newVolumes)) volumes = newVolumes
        const avg = _average(newVolumes)
        if (volume !== avg) volume = avg
        if (muted !== newMuted) muted = newMuted
        _syncing = false
    }

    onMutedChanged: if (!_syncing) PwCore.setMuted(_node, muted)
    onVolumesChanged: if (!_syncing) PwCore.setVolumes(_node, Array.from(volumes, v => Math.max(0, v)))
    onVolumeChanged: {
        if (_syncing) return
        // Same scaling as quickshell: keep the channel balance, or set every channel when silent.
        const old = _average(volumes)
        const mul = old === 0 ? 0 : volume / old
        PwCore.setVolumes(_node, Array.from(volumes, v => Math.max(0, mul === 0 ? volume : v * mul)))
    }
}
