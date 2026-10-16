import QtQuick
import Quickshell
import Quickshell.Io

// `ipc wait` and `ipc listen` against this same instance: void and typed signals, values in
// upstream's formats, unknown target/signal errors, listeners ending with their client.
ShellRoot {
    id: root
    property var errors: []
    property var lines: []
    property string waited: ""
    property int step: 0
    property int ticks: 0
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }
    function client(args) {
        return ["/proc/" + Quickshell.processId + "/exe", "-p", Quickshell.shellPath("ipcsignals.qml"), "ipc"].concat(args)
    }

    IpcHandler {
        id: handler
        target: "sig"
        signal ping()
        signal said(text: string)
        signal counted(n: int)
        signal flagged(on: bool)
        signal tinted(c: color)
        function noop(): void {}
    }

    Process {
        id: listener
        command: root.client(["listen", "sig", "said"])
        stdout: SplitParser { onRead: line => root.lines.push(line) }
    }
    Process {
        id: waiter
        command: root.client(["wait", "sig", "ping"])
        stdout: StdioCollector { onStreamFinished: root.waited = text }
    }
    Process { id: counter; property string out; command: root.client(["wait", "sig", "counted"]); stdout: StdioCollector { onStreamFinished: counter.out = text } }
    Process { id: flagger; property string out; command: root.client(["wait", "sig", "flagged"]); stdout: StdioCollector { onStreamFinished: flagger.out = text } }
    Process { id: tinter; property string out; command: root.client(["wait", "sig", "tinted"]); stdout: StdioCollector { onStreamFinished: tinter.out = text } }
    Process { id: badSignal; property int code: -1; property string err; command: root.client(["wait", "sig", "nope"]); stderr: StdioCollector { onStreamFinished: badSignal.err = text } onExited: c => code = c }
    Process { id: badTarget; property int code: -1; property string err; command: root.client(["wait", "zzz", "ping"]); stderr: StdioCollector { onStreamFinished: badTarget.err = text } onExited: c => code = c }

    Component.onCompleted: {
        listener.running = true
        waiter.running = true
        counter.running = true
        flagger.running = true
        tinter.running = true
        badSignal.running = true
        badTarget.running = true
    }

    Timer {
        interval: 100
        repeat: true
        running: true
        onTriggered: {
            if (++root.ticks > 80) { root.check("timeout at step " + root.step, false); root.finish(); return }
            switch (root.step) {
            case 0:
                // Emit until the clients have connected and got their value.
                handler.said("first line")
                handler.ping()
                handler.counted(42)
                handler.flagged(true)
                handler.tinted("#80ff0000")
                if (root.lines.length < 1 || waiter.running || counter.running || flagger.running || tinter.running
                    || badSignal.code < 0 || badTarget.code < 0) return
                root.check("wait void " + root.waited, root.waited === "void\n")
                root.check("wait int " + counter.out, counter.out === "42\n")
                root.check("wait bool " + flagger.out, flagger.out === "true\n")
                root.check("wait color " + tinter.out, tinter.out === "#80ff0000\n")
                root.check("listen string", root.lines[0] === "first line")
                root.check("unknown signal " + badSignal.err, badSignal.code !== 0 && badSignal.err.indexOf("Signal not found.") >= 0)
                root.check("unknown target " + badTarget.err, badTarget.code !== 0 && badTarget.err.indexOf("Target not found.") >= 0)
                root.lines = []
                handler.said("second")
                handler.said("third")
                root.step = 1
                break
            case 1:
                // Step 0 emitted "first line" on every tick; late copies may still arrive.
                const rest = root.lines.filter(l => l !== "first line")
                if (rest.length < 2) return
                root.check("listen keeps streaming " + JSON.stringify(rest), rest[0] === "second" && rest[1] === "third")
                root.check("listener still running", listener.running)
                listener.running = false
                root.step = 2
                break
            case 2:
                if (listener.running) return
                // The dropped client must not break later emissions.
                handler.said("after")
                root.finish()
            }
        }
    }
}
