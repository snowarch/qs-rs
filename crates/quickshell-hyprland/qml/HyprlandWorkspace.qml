import QtQml

QtObject {
    property int id: -1
    property string name
    property bool active: false
    property bool focused: false
    property bool urgent: false
    property bool hasFullscreen: false
    property var lastIpcObject: ({})
    property QtObject monitor: null
    property QtObject toplevels: QtObject { property list<QtObject> values }
    function activate() { Hyprland.dispatch("workspace " + (id > 0 ? id : "name:" + name)) }
}
