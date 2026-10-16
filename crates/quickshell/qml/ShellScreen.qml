import QtQuick

// One monitor, as exposed by Quickshell.screens. Wraps the QQuickScreenInfo that Window.screen
// accepts, keeping Quickshell's property names.
QtObject {
    required property var _info
    readonly property string name: _info ? _info.name : ""
    readonly property string model: _info ? _info.model : ""
    readonly property string serialNumber: _info ? _info.serialNumber : ""
    readonly property int x: _info ? _info.virtualX : 0
    readonly property int y: _info ? _info.virtualY : 0
    readonly property int width: _info ? _info.width : 0
    readonly property int height: _info ? _info.height : 0
    readonly property real physicalPixelDensity: _info ? _info.pixelDensity : 0
    readonly property real logicalPixelDensity: _info ? _info.logicalPixelDensity : 0
    readonly property real devicePixelRatio: _info ? _info.devicePixelRatio : 1
    readonly property int orientation: _info ? _info.orientation : 0
    readonly property int primaryOrientation: _info ? _info.primaryOrientation : 0
    function toString() { return "ShellScreen(" + name + ")" }
}
