import QtQml
import Quickshell.Services.Pipewire

QtObject {
    property int id: -1
    property string name
    property string description
    property string nickname
    property int type: PwNodeType.Untracked
    readonly property bool isSink: (type & PwNodeType.Sink) !== 0
    readonly property bool isStream: (type & PwNodeType.Stream) !== 0
    property var properties: ({})
    property PwNodeAudio audio: null
    property bool ready: false
    property bool _isMonitor: false
}
