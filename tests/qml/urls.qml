import QtQuick
import Quickshell
import "root:urlslib"
import "root:/urlslib/util.js" as Util

// Quickshell URL rules: `root:` imports resolve from the shell root, and LazyLoader.source is
// relative to the file that sets it (not to LazyLoader's own file).
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }

    Thing { id: thing }
    LazyLoader { id: lazy; source: "urlslib/Thing.qml"; active: true }

    Timer {
        interval: 200
        running: true
        onTriggered: {
            root.check("root: directory import", thing.value === 42)
            root.check("root: script import", Util.answer() === 7)
            root.check("lazy relative source", lazy.item !== null && lazy.item.value === 42)
            if (root.errors.length) console.log("FAILED: " + root.errors.join(", "))
            Qt.exit(root.errors.length ? 1 : 0)
        }
    }
}
