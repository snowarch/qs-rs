import QtQml
import Quickshell.Services.Pipewire

QtObject {
    property int id: -1
    property PwNode source: null
    property PwNode target: null
    property int state: PwLinkState.Unlinked
    property int _out: -1
    property int _in: -1
}
