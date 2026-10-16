import QtQuick
import Quickshell.Io

// Exercises Process + StdioCollector + SplitParser against real child processes.
QtObject {
    id: root
    property var lines: []
    property int done: 0
    property var errors: []

    function check(name, ok) {
        if (!ok) errors.push(name)
        if (++done === 6) {
            if (errors.length) console.log("FAILED: " + errors.join(", "))
            Qt.exit(errors.length ? 1 : 0)
        }
    }

    property Process collect: Process {
        command: ["sh", "-c", "printf 'hello\\nworld'; exit 3"]
        running: true
        stdout: StdioCollector { id: col }
        onExited: (exitCode, exitStatus) => root.check("collect", col.text === "hello\nworld" && exitCode === 3 && exitStatus === 0)
    }

    property Process split: Process {
        command: ["sh", "-c", "printf 'a\\nb\\nc'"]
        running: true
        stdout: SplitParser { onRead: data => root.lines.push(data) }
        onExited: root.check("split", root.lines.join("|") === "a|b|c")
    }

    property Process env: Process {
        command: ["sh", "-c", "printf '%s:%s' \"$FOO\" \"${HOME:-unset}\""]
        environment: ({ FOO: "bar", HOME: null })
        running: true
        stdout: StdioCollector { id: envOut }
        onExited: root.check("env", envOut.text === "bar:unset")
    }

    property Process stdin: Process {
        command: ["cat"]
        stdinEnabled: true
        running: true
        stdout: StdioCollector { id: catOut }
        onStarted: { write("piped"); stdinEnabled = false }
        onExited: root.check("stdin", catOut.text === "piped")
    }

    property Process execd: Process {
        id: execd
        stdout: StdioCollector { id: execOut }
        onExited: root.check("exec", execOut.text === "via exec")
    }

    property Process killed: Process {
        command: ["sleep", "30"]
        running: true
        onStarted: running = false
        onExited: (code, status) => root.check("terminate", status === 1 && code === 15 && !running && processId === null)
    }

    Component.onCompleted: execd.exec({ command: ["echo", "-n", "via exec"] })
}
