import QtQml

QtObject {
    property int id: -1
    property string name
    property string description
    property int x
    property int y
    property int width
    property int height
    property real scale: 1
    property var lastIpcObject: ({})
    property QtObject activeWorkspace: null
    property bool focused: false
}
