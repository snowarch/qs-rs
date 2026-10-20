import QtQuick
import Quickshell
import Quickshell.Io

// inir's job queues (Wallpapers' playback copies): start the next job only when the process is
// not running, and chain from onExited. As upstream, `running = true` after load starts at once
// and reads true, so two jobs asked for in one turn both run (the second used to overwrite the
// first, and a live wallpaper stayed black for good).
ShellRoot {
    id: root
    property var queue: []
    property var done: []
    property bool readsRunning: false
    function run() {
        if (proc.running || root.queue.length === 0) return
        const job = root.queue.shift()
        proc.job = job
        proc.command = ["sh", "-c", "exit 0"]
        proc.running = true
        if (job === "a") root.readsRunning = proc.running
    }
    function request(job) { root.queue.push(job); root.run() }
    Process {
        id: proc
        property string job
        onExited: { root.done.push(proc.job); root.run() }
    }
    Timer {
        interval: 100; running: true
        onTriggered: { root.request("a"); root.request("b"); root.request("c") }
    }
    Timer {
        interval: 2000; running: true
        onTriggered: {
            const ok = root.done.join() === "a,b,c" && root.readsRunning
            if (!ok) console.log("FAILED: done=" + root.done.join() + " readsRunning=" + root.readsRunning)
            Qt.exit(ok ? 0 : 1)
        }
    }
}
