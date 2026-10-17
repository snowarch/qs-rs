import QtQuick
import Quickshell

// Variants with an Item delegate (inir's Dock and RegionSelector put a Loader there): instances
// get modelData, are not reported as "not placed in the graphics scene" (wrap.sh greps the
// log), survive JS garbage collection and die with their Variants.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) errors.push(name) }

    LazyLoader {
        id: holder
        active: true
        Variants {
            model: ["a", "b"]
            Loader {
                required property string modelData
                property string got: modelData
                sourceComponent: QtObject {}
                Component.onDestruction: root.destroyedCount++
            }
        }
    }

    property int destroyedCount: 0

    Timer {
        interval: 50
        running: true
        onTriggered: {
            const v = holder.item
            root.check("two instances", v.instances.length === 2)
            root.check("modelData", v.instances.map(i => i.got).join() === "a,b")
            root.check("loader loaded", v.instances.every(i => i.item !== null))
            gc()
            root.check("survive gc", v.instances.length === 2 && v.instances[0].got === "a")
            holder.active = false
            after.start()
        }
    }

    Timer {
        id: after
        interval: 50
        onTriggered: {
            root.check("destroyed with owner", root.destroyedCount === 2)
            if (root.errors.length) { console.log("FAIL", root.errors.join(", ")); Qt.exit(1) }
            else Qt.exit(0)
        }
    }
}
