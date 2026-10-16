import QtQuick
import Quickshell

// ScriptModel turns array updates into row inserts/removes/moves/updates, so delegates are kept:
// creations are counted per update. Covers structural equality, objectProp keys and Identity mode.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    property int created: 0
    property var ops: []

    ScriptModel { id: model; values: ["a", "b", "c"] }
    Connections {
        target: model
        function onRowsInserted(p, first, last) { root.ops.push(`ins ${first}-${last}`) }
        function onRowsRemoved(p, first, last) { root.ops.push(`rm ${first}-${last}`) }
        function onRowsMoved(p, s, e, d, row) { root.ops.push(`mv ${s}-${e}>${row}`) }
        function onDataChanged(a, b) { root.ops.push(`dt ${a.row}-${b.row}`) }
    }
    Item {
        Repeater {
            id: rep
            model: model
            delegate: Item { required property var modelData; Component.onCompleted: root.created++ }
        }
    }
    function texts() { const r = []; for (let i = 0; i < rep.count; i++) r.push(JSON.stringify(rep.itemAt(i).modelData)); return r.join(",") }
    function step(name, values, wantCreated, wantOps, wantTexts) {
        created = 0; ops = []
        model.values = values
        check(name + " created " + created, created === wantCreated)
        check(name + " ops " + ops.join("|"), ops.join("|") === wantOps)
        check(name + " texts " + texts(), texts() === wantTexts)
    }

    Component.onCompleted: {
        check("initial", rep.count === 3 && model.rowCount() === 3 && created === 3)
        step("append", ["a", "b", "c", "d"], 1, "ins 3-3", '"a","b","c","d"')
        step("remove middle", ["a", "c", "d"], 0, "rm 1-1", '"a","c","d"')
        step("move", ["d", "a", "c"], 0, "mv 2-2>0", '"d","a","c"')
        step("insert front", ["x", "d", "a", "c"], 1, "ins 0-0", '"x","d","a","c"')
        step("structural", [{ n: 1 }, { n: 2 }], 2, "ins 0-1|rm 2-5", '{"n":1},{"n":2}')
        step("structural same", [{ n: 1 }, { n: 2 }], 0, "dt 0-1", '{"n":1},{"n":2}')
        model.objectProp = "id"
        step("keyed update", [{ id: 1, v: "a" }, { id: 2, v: "b" }], 2, "ins 0-1|rm 2-3", '{"id":1,"v":"a"},{"id":2,"v":"b"}')
        step("keyed change", [{ id: 1, v: "a2" }, { id: 2, v: "b" }], 0, "dt 0-1", '{"id":1,"v":"a2"},{"id":2,"v":"b"}')
        model.objectProp = ""
        model.comparisonMode = ObjectComparison.Identity
        step("identity", [{ id: 1, v: "a2" }], 1, "ins 0-0|rm 1-2", '{"id":1,"v":"a2"}')
        check("values read back", model.values.length === 1 && model.values[0].v === "a2")
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }
}
