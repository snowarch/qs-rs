import QtQml

QtObject {
    property string address
    property string title
    property bool activated: false
    property bool urgent: false
    property var lastIpcObject: ({})
    property QtObject workspace: null
    property QtObject monitor: null
    property QtObject wayland: null
}
