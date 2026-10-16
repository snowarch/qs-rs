import QtQuick
import Quickshell

// ShellRoot, Scope, Variants and LazyLoader behaviour.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) errors.push(name) }

    Scope {
        id: scope
        QtObject { objectName: "a" }
        QtObject { objectName: "b" }
    }

    Variants {
        id: variants
        model: ["x", "y"]
        QtObject { required property string modelData }
    }

    LazyLoader {
        id: sync
        QtObject { property int v: 7 }
    }

    LazyLoader {
        id: async
        loading: true
        QtObject { property int v: 9 }
    }

    Timer {
        interval: 50; running: true
        onTriggered: {
            root.check("scope", scope.children.length === 2)
            root.check("variants.initial", variants.instances.length === 2 && variants.instances[1].modelData === "y")
            const kept = variants.instances[0]
            variants.model = ["x", "z"]
            root.check("variants.keep", variants.instances[0] === kept && variants.instances[1].modelData === "z")
            root.check("lazy.inactive", sync.item === null && !sync.active)
            sync.active = true
            root.check("lazy.active", sync.item !== null && sync.item.v === 7)
            sync.active = false
            root.check("lazy.destroy", sync.item === null)
            root.check("lazy.async", async.active && async.item.v === 9 && !async.loading)
            root.check("shellDir", Quickshell.shellDir.endsWith("/tests/qml"))
            root.check("dataDir", Quickshell.dataDir.indexOf("/quickshell/by-shell/") > 0)
            root.check("iconPath", Quickshell.iconPath("foo") === "image://icon/foo" && Quickshell.iconPath("foo", "bar") === "image://icon/foo?fallback=bar")
            root.check("hasVersion", Quickshell.hasVersion(0, 2) && !Quickshell.hasVersion(9, 0))
            if (root.errors.length) console.log("FAILED: " + root.errors.join(", "))
            Qt.exit(root.errors.length ? 1 : 0)
        }
    }
}
