import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Polkit

// Another agent holding the session (a nested shell on the same system bus) does not leave this
// one unregistered for good: it retries until polkitd takes it.
ShellRoot {
    id: root
    property var st: null
    Process {
        id: stateProc
        command: ["gdbus", "call", "--system", "-d", "org.qsrs.Mock", "-o", "/org/qsrs/Mock", "-m", "org.qsrs.Mock.State"]
        stdout: StdioCollector { onStreamFinished: root.st = JSON.parse(text.slice(text.indexOf("'") + 1, text.lastIndexOf("'"))) }
    }
    PolkitAgent { id: agent }
    property int ticks: 0
    Timer {
        interval: 100; running: true; repeat: true
        onTriggered: {
            root.ticks++
            if (root.ticks > 90) { console.log("FAILED: timeout registered=" + agent.isRegistered + " " + JSON.stringify(root.st)); Qt.exit(1) }
            if (!agent.isRegistered) return
            if (!root.st) { if (!stateProc.running) stateProc.running = true; return }
            const ok = root.st.attempts === 2 && root.st.registered.length === 1
            if (!ok) console.log("FAILED: " + JSON.stringify(root.st))
            Qt.exit(ok ? 0 : 1)
        }
    }
}
