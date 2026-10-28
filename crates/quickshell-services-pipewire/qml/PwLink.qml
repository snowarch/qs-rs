import QtQml
import Quickshell.Services.Pipewire

QtObject {
    property int id: -1
    property PwNode source: null
    property PwNode target: null
    // PwLinkState.Unlinked: a negative enum key in a binding makes qmlcachegen 6.12 write C++ that warns
    property int state: -1
    property int _out: -1
    property int _in: -1
}
