import QtQuick
import Quickshell

// A regular toplevel window.
ProxyWindowBase {
    property string title: ""
    property size minimumSize: Qt.size(0, 0)
    property size maximumSize: Qt.size(0, 0)
    _window.title: title
    _window.minimumWidth: minimumSize.width
    _window.minimumHeight: minimumSize.height
    _window.maximumWidth: maximumSize.width > 0 ? maximumSize.width : 16777215
    _window.maximumHeight: maximumSize.height > 0 ? maximumSize.height : 16777215
}
