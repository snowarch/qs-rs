import QtQuick
import Quickshell

// A service object that leaves its model while a delegate holding it is still incubating must
// outlive that incubation. Qt snapshots a JS-array model's entry into a QVariantMap with raw
// QObject pointers when it creates the delegate and writes it into `modelData` ticks later; a
// destroy() in between made the write wrap a freed object (SIGSEGV switching inir's Settings
// from a window to the overlay while a toplevel closed, 2026-10-21). Quickshell._retire keeps
// the object until consumers that rebuild on a timer have dropped it.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }

    property Component objComponent: Component { QtObject { property string name: "t" } }
    property var held: []
    property var apps: []
    property int wrote: 0

    // The consumer: wraps each object in a map and rebuilds on a 100 ms timer, as inir's
    // CompositorService (sortedToplevels) and TaskbarApps do.
    function build() { return root.held.map(o => ({ appId: o.name, toplevels: [{ handle: o }] })) }
    Timer { id: rebuild; interval: 100; onTriggered: root.apps = root.build() }

    Loader {
        id: loader
        active: false
        asynchronous: true
        sourceComponent: Column {
            Repeater {
                model: root.apps
                delegate: Item {
                    required property var modelData
                    width: 4; height: 4
                    Component.onCompleted: if (++root.wrote === 100) root.drop()
                }
            }
        }
    }

    Component.onCompleted: {
        for (let i = 0; i < 3000; i++) held.push(objComponent.createObject(root, { name: "t" + i }))
        apps = build()
        loader.active = true
    }
    // A hundred delegates in, every object leaves the model the way services drop them, while the
    // rest of the delegates already hold their entries.
    function drop() {
        const gone = root.held
        root.held = []
        rebuild.restart()
        for (const o of gone) Quickshell._retire(o)
        finish.start()
    }
    Timer {
        id: finish
        interval: 1500
        onTriggered: {
            root.check("loader ready", loader.status === Loader.Ready)
            root.check("model emptied", root.apps.length === 0)
            if (root.errors.length === 0) Qt.exit(0); else Qt.exit(1)
        }
    }
}
