import QtQml
import Quickshell

// Upstream QsMenuEntry: one menu item, also a handle for its own submenu. `opened`/`closed` follow
// the reference count QsMenuOpener keeps (ref/unref).
QsMenuHandle {
    id: root
    menu: root
    property bool isSeparator: false
    property bool enabled: true
    property string text
    property string icon
    property int buttonType: QsMenuButtonType.None
    property int checkState: Qt.Unchecked
    property bool hasChildren: false
    readonly property ObjectModel children: ObjectModel {}

    signal triggered()
    signal opened()
    signal closed()

    property int _qsRefs: 0
    function ref() { if (++_qsRefs === 1) opened() }
    function unref() { if (--_qsRefs === 0) closed() }
    function display(parentWindow, relativeX, relativeY) {
        console.warn("QsMenuEntry.display is not supported by qs-rs yet")
    }
}
