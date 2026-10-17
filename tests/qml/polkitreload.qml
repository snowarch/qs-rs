import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Polkit

// The agent registration outlives a reload, as upstream's PolkitAgentImpl::instance(): the new
// generation's PolkitAgent is registered at once, and polkitd saw one registration and no
// unregistration (a second one would race the first: "an agent already exists").
ShellRoot {
    id: root
    FileView { id: marker; path: Quickshell.statePath("polkitreload-generation"); blockLoading: true; printErrors: false }
    readonly property int generation: parseInt(marker.text() || "0")
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
            if (root.ticks > 60) { console.log("FAILED: timeout registered=" + agent.isRegistered + " " + JSON.stringify(root.st)); Qt.exit(1) }
            if (!agent.isRegistered) return
            if (root.generation === 0) {
                marker.setText("1")
                running = false
                Quickshell.reload(false)
                return
            }
            if (!root.st) { if (!stateProc.running) stateProc.running = true; return }
            const ok = root.st.registered.length === 1 && root.st.unregistered === 0
            if (!ok) console.log("FAILED: " + JSON.stringify(root.st))
            Qt.exit(ok ? 0 : 1)
        }
    }
}
