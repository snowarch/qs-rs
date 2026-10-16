import QtQuick
import Quickshell
import Quickshell.Io

// IpcHandler reached through the `ipc call` / `ipc show` client of this same binary and, when
// installed, through upstream Quickshell's own `qs` client (same instance layout and protocol).
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

    function run(args, onDone, bin) {
        const p = Qt.createQmlObject('import Quickshell.Io; Process { stdout: StdioCollector {} stderr: StdioCollector {} }', root)
        p.command = [bin ?? "/proc/" + Quickshell.processId + "/exe", "-p", Quickshell.shellPath("ipc.qml"), "ipc"].concat(args)
        p.exited.connect(code => onDone(code, p.stdout.text, p.stderr.text))
        p.running = true
    }

    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }

    function upstream() {
        const qs = "/usr/bin/qs"
        const probe = Qt.createQmlObject('import Quickshell.Io; Process { command: ["test", "-x", "/usr/bin/qs"] }', root)
        probe.exited.connect(code => {
            if (code !== 0) { console.log("upstream qs not installed, skipping its checks"); finish(); return }
            run(["call", "test", "greet", "ann", "1", "false"], (c, out, err) => {
                check("qs.call", c === 0 && out.trim() === "hi ann" && root.got === "ann1false")
                run(["call", "test", "noop"], (c2, out2, err2) => {
                    check("qs.void", c2 === 0 && out2 === "")
                    run(["call", "test", "missing"], (c3, out3, err3) => {
                        // upstream's client prints errors on stdout
                        check("qs.missing", (out3 + err3).indexOf("Function not found.") >= 0)
                        run(["show"], (c4, out4) => {
                            check("qs.show", c4 === 0 && out4.indexOf("function greet(name: string, times: int, loud: bool): string") >= 0)
                            finish()
                        }, qs)
                    }, qs)
                }, qs)
            }, qs)
        })
        probe.running = true
    }

    Component.onCompleted: run(["call", "test", "greet", "bob", "3", "true"], (code, out) => {
        check("call", code === 0 && out.trim() === "hi bob" && root.got === "bob3true")
        run(["call", "test", "greet", "bob", "x", "true"], (code2, out2, err2) => {
            check("badint", code2 !== 0 && err2.indexOf("Unable to parse argument 2 as int") >= 0)
            run(["show"], (code3, out3) => {
                check("show", code3 === 0 && out3.indexOf("function greet(name: string, times: int, loud: bool): string") >= 0)
                upstream()
            })
        })
    })
}
