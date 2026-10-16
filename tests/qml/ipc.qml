import QtQuick
import Quickshell
import Quickshell.Io

// IpcHandler reached through the real `ipc call` / `ipc show` client of this same binary.
ShellRoot {
    id: root
    property var errors: []
    property string got: ""
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }

    IpcHandler {
        target: "test"
        function greet(name: string, times: int, loud: bool): string {
            root.got = name + times + loud
            return "hi " + name
        }
        function noop(): void {}
    }

    function run(args, onDone) {
        const p = Qt.createQmlObject('import Quickshell.Io; Process { stdout: StdioCollector {} stderr: StdioCollector {} }', root)
        p.command = ["/proc/" + Quickshell.processId + "/exe", "-p", Quickshell.shellPath("ipc.qml"), "ipc"].concat(args)
        p.exited.connect(code => onDone(code, p.stdout.text, p.stderr.text))
        p.running = true
    }

    Component.onCompleted: run(["call", "test", "greet", "bob", "3", "true"], (code, out) => {
        check("call", code === 0 && out.trim() === "hi bob" && root.got === "bob3true")
        run(["call", "test", "greet", "bob", "x", "true"], (code2, out2, err2) => {
            check("badint", code2 !== 0 && err2.indexOf("not an int") >= 0)
            run(["show"], (code3, out3) => {
                check("show", code3 === 0 && out3.indexOf("function greet(name: string, times: int, loud: bool): string") >= 0)
                if (errors.length) console.log("FAILED: " + errors.join(", "))
                Qt.exit(errors.length ? 1 : 0)
            })
        })
    })
}
