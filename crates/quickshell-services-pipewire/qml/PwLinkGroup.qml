import QtQml
import Quickshell.Services.Pipewire

// All links between the same output and input node; the state is the first link's.
QtObject {
    property PwNode source: null
    property PwNode target: null
    // PwLinkState.Unlinked: a negative enum key in a binding makes qmlcachegen 6.12 write C++ that warns
    property int state: -1
    property int _outputNode: -1
    property int _inputNode: -1
    property var _links: []
}
