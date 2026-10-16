import QtQuick
import Quickshell

// LazyLoader keeps bindings on `active` (upstream's setters never replace them) and creates its
// item once, after the shell finished loading, nested loaders included.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) errors.push(name) }
    property bool flag: true
    property int created: 0
    property int nestedCreated: 0
    property int destroyed: 0

    LazyLoader {
        id: bound
        active: root.flag
        Scope {
            Component.onCompleted: root.created++
            Component.onDestruction: root.destroyed++
            LazyLoader {
                active: true
                QtObject { Component.onCompleted: root.nestedCreated++ }
            }
        }
    }

    LazyLoader {
        id: viaSource
        active: true
        source: "scopelib/Reservations.qml"
    }

    Timer {
        interval: 50; running: true
        onTriggered: {
            root.check("startup.once", root.created === 1 && root.destroyed === 0)
            root.check("nested.once", root.nestedCreated === 1)
            root.check("source", viaSource.item !== null)
            root.flag = false
            root.check("binding.off", !bound.active && bound.item === null)
            root.flag = true
            root.check("binding.on", bound.active && bound.item !== null && root.created === 2)
            if (root.errors.length) console.log("FAILED: " + root.errors.join(", "))
            Qt.exit(root.errors.length ? 1 : 0)
        }
    }
}
