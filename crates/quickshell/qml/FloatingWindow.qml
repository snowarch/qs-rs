import QtQuick
import Quickshell

// A regular toplevel window.
ProxyWindowBase {
    id: root
    property string title: ""
    property size minimumSize: Qt.size(0, 0)
    property size maximumSize: Qt.size(0, 0)
    _reuseTag: "floating"
    property list<QtObject> _floatingBindings: [
        Binding { target: root._window; property: "title"; value: root.title; restoreMode: Binding.RestoreNone },
        Binding { target: root._window; property: "minimumWidth"; value: root.minimumSize.width; restoreMode: Binding.RestoreNone },
        Binding { target: root._window; property: "minimumHeight"; value: root.minimumSize.height; restoreMode: Binding.RestoreNone },
        Binding {
            target: root._window
            property: "maximumWidth"
            value: root.maximumSize.width > 0 ? root.maximumSize.width : 16777215
            restoreMode: Binding.RestoreNone
        },
        Binding {
            target: root._window
            property: "maximumHeight"
            value: root.maximumSize.height > 0 ? root.maximumSize.height : 16777215
            restoreMode: Binding.RestoreNone
        }
    ]
}
