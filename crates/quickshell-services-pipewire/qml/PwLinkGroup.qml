import QtQml
import Quickshell.Services.Pipewire

// All links between the same output and input node; the state is the first link's.
QtObject {
    property PwNode source: null
    property PwNode target: null
    property int state: PwLinkState.Unlinked
    property int _outputNode: -1
    property int _inputNode: -1
    property var _links: []
}
