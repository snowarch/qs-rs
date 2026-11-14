import QtQuick
import Quickshell

// A file only lazyloader.qml's background loads compile, so its first load is a real one, not the type cache's.
Scope {
    property int level: 2
    QtObject { property string name: "deferred" }
}
