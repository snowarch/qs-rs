import QtQuick
import Quickshell

// ObjectModel works as a model (one delegate per object, modelData) and as a JS array, and an
// assignment of `values` only inserts/removes what changed, so kept objects keep their delegate.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    property int created: 0
    property var events: []

    QtObject { id: a; property string name: "a" }
    QtObject { id: b; property string name: "b" }
    QtObject { id: c; property string name: "c" }

    ObjectModel { id: model }
    Connections {
        target: model
        function onObjectInsertedPost(o, i) { root.events.push(`+${o.name}@${i}`) }
        function onObjectRemovedPost(o, i) { root.events.push(`-${o.name}@${i}`) }
    }
    Instantiator {
        id: inst
        model: model
        delegate: QtObject { required property QtObject modelData; Component.onCompleted: root.created++ }
    }
    function names() { const r = []; for (let i = 0; i < inst.count; i++) r.push(inst.objectAt(i).modelData.name); return r.join("") }

    Component.onCompleted: {
        model.values = [a, b]
        check("two delegates " + names(), inst.count === 2 && names() === "ab" && created === 2)
        check("array filter", model.values.filter(o => o.name === "b").length === 1 && model.values.length === 2)
        events = []; created = 0
        model.values = [b, a, c]
        check("reorder+append " + names() + " " + events.join(","), names() === "bac" && created === 2 && events.join(",") === "-b@1,+b@0,+c@2")
        events = []; created = 0
        model.values = model.values.filter(o => o !== a)
        check("remove " + names() + " " + events.join(","), names() === "bc" && created === 0 && events.join(",") === "-a@1")
        check("indexOf", model.indexOf(c) === 1 && model.indexOf(a) === -1)
        console.log(errors.length ? "FAIL " + errors.join(", ") : "PASS")
        Qt.exit(errors.length ? 1 : 0)
    }
}
