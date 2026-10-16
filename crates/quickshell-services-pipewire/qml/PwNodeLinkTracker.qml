import QtQml
import Quickshell.Services.Pipewire

// Link groups ending at `node` when it is a sink, or starting at it otherwise. Groups whose
// target is a monitor node are left out.
QtObject {
    property PwNode node: null
    readonly property list<PwLinkGroup> linkGroups: {
        const n = node
        if (!n) return []
        return Pipewire.linkGroups.values.filter(g => (n.isSink ? g._inputNode : g._outputNode) === n.id
                                                      && !(g.target && g.target._isMonitor))
    }
}
