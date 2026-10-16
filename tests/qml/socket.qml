import QtQuick
import Quickshell
import Quickshell.Io

// Socket against a python echo server started by Process: connect, line parsing, write,
// connection errors, remote close and reconnect by setting connected again.
ShellRoot {
    id: root
    property var errors: []
    property int step: 0
    property int ticks: 0
    property var lines: []
    property var errs: []
    property int states: 0
    readonly property string path: Quickshell.env("XDG_RUNTIME_DIR") + "/echo.sock"
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        server.running = false
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }

    // Echoes lines back; "bye" closes the connection from the server side.
    Process {
        id: server
        running: true
        command: ["python3", "-c", "import socket,os,sys,time\np=sys.argv[1]\ntry: os.unlink(p)\nexcept OSError: pass\ns=socket.socket(socket.AF_UNIX); s.bind(p); s.listen(); print('ready', flush=True)\nwhile True:\n c,_=s.accept(); c.sendall(b'hel'); time.sleep(0.05); c.sendall(b'lo\\n')\n for l in c.makefile():\n  if l.strip()=='bye': break\n  c.sendall(('echo '+l).encode())\n c.close()", root.path]
        stdout: SplitParser { onRead: line => { if (line === "ready") root.step = 1 } }
    }

    Socket {
        id: missing
        path: root.path + ".none"
        onError: e => root.errs.push(e)
    }

    Socket {
        id: sock
        path: root.path
        parser: SplitParser { onRead: line => root.lines.push(line) }
        onConnectionStateChanged: root.states++
        onError: e => root.errs.push(e)
    }

    Timer {
        interval: 50
        repeat: true
        running: true
        onTriggered: {
            root.ticks++
            if (root.ticks > 120) { root.check("timeout at step " + root.step, false); root.finish(); return }
            switch (root.step) {
            case 1:
                missing.connected = true
                sock.connected = true
                root.step = 2
                break
            case 2:
                if (!sock.connected || root.lines.length < 1) return
                root.check("missing socket error", root.errs.includes(2) && !missing.connected)
                root.check("greeting", root.lines[0] === "hello")
                sock.write("one\ntw")
                sock.write("o\n")
                sock.flush()
                root.step = 3
                break
            case 3:
                if (root.lines.length < 3) return
                root.check("echoes " + JSON.stringify(root.lines), root.lines[1] === "echo one" && root.lines[2] === "echo two")
                sock.write("bye\n")
                root.step = 4
                break
            case 4:
                if (sock.connected) return
                root.check("closed by peer", root.states === 2 && root.errs.includes(1))
                sock.connected = true
                root.step = 5
                break
            case 5:
                if (!sock.connected || root.lines.length < 4) return
                root.check("reconnected greeting", root.lines[3] === "hello")
                sock.connected = false
                root.step = 6
                break
            case 6:
                if (sock.connected) return
                root.check("client close", root.states === 4)
                sock.write("ignored\n")
                root.finish()
            }
        }
    }
}
