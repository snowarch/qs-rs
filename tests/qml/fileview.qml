import QtQuick
import Quickshell
import Quickshell.Io

// FileView reads/writes, binding-aware text(), watchChanges and JsonAdapter round trips.
ShellRoot {
    id: root
    property var errors: []
    property int step: 0
    readonly property string dir: Quickshell.cachePath("fv-test")
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }

    FileView {
        id: plain
        path: root.dir + "/plain.txt"
        blockWrites: true
        printErrors: false
    }
    property string boundText: plain.text()

    FileView {
        id: missing
        path: root.dir + "/does-not-exist"
        printErrors: false
        property int failed: -1
        onLoadFailed: error => failed = error
    }

    FileView {
        id: watched
        path: ""
        watchChanges: true
        printErrors: false
        property int changes: 0
        onFileChanged: changes++
    }

    FileView {
        id: config
        path: root.dir + "/config.json"
        blockLoading: true
        printErrors: false
        property int updates: 0
        onAdapterUpdated: { updates++; writeAdapter() }

        JsonAdapter {
            id: adapter
            property string name: "default"
            property int count: 1
            property var list: [1, 2]
            property JsonObject nested: JsonObject {
                property bool flag: false
            }
        }
    }

    Process {
        id: touch
        command: ["sh", "-c", "mkdir -p '" + root.dir + "' && printf 'x' > '" + root.dir + "/watched.txt' && printf '{\"name\":\"fromdisk\",\"nested\":{\"flag\":true}}' > '" + root.dir + "/preset.json'"]
        running: true
    }

    FileView {
        id: preset
        path: ""
        printErrors: false
        JsonAdapter {
            id: presetAdapter
            property string name: "unset"
            property JsonObject nested: JsonObject { property bool flag: false }
        }
    }

    Timer {
        interval: 150; running: true; repeat: true
        onTriggered: {
            switch (root.step++) {
            case 0:
                plain.setText("hello")
                root.check("write.sync", plain.text() === "hello")
                root.check("binding", root.boundText === "hello")
                root.check("missing", missing.failed === FileViewError.FileNotFound && !missing.loaded)
                adapter.name = "changed"
                preset.path = root.dir + "/preset.json"
                watched.path = root.dir + "/watched.txt"
                break
            case 1:
                root.check("adapter.updated", config.updates >= 1)
                const saved = JSON.parse(config.text())
                root.check("adapter.write", saved.name === "changed" && saved.count === 1 && saved.nested.flag === false && saved.list.length === 2)
                root.check("adapter.read", presetAdapter.name === "fromdisk" && presetAdapter.nested.flag === true)
                const p = Qt.createQmlObject('import Quickshell.Io; Process { command: ["sh", "-c", "printf y >> \'' + root.dir + '/watched.txt\'"]; running: true }', root)
                break
            case 2:
                root.check("watch", watched.changes >= 1)
                root.finish()
            }
        }
    }
}
