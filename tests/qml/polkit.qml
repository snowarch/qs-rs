import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Polkit

// Runs against tests/polkit/mock.py (fake polkitd, logind and agent helper socket) on a private
// system bus: registration subject, request properties and identities, prompts, a wrong password
// and the retry, identity switch, success, the queue, user and daemon cancel, a request without
// usable identities, a caller that is not polkitd, and a second agent that must stay idle.
ShellRoot {
    id: root
    property var errors: []
    property int step: 0
    property int ticks: 0
    property var st: null
    property int started: 0
    property int succeeded: 0
    property int failedSignals: 0
    property int daemonCancels: 0
    property var supplementary: []
    property var ended: []
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }
    function mock(method, args) {
        callComponent.createObject(root, { command: ["gdbus", "call", "--system", "-d", "org.qsrs.Mock", "-o", "/org/qsrs/Mock",
                                                      "-m", "org.qsrs.Mock." + method].concat(args) })
    }
    function result(cookie) { return st?.results[cookie] ?? "" }
    function cancelled(cookie) { return result(cookie).startsWith("org.freedesktop.PolicyKit1.Error.Cancelled") }

    Component { id: callComponent; Process { running: true; onExited: destroy() } }
    Process {
        id: stateProc
        command: ["gdbus", "call", "--system", "-d", "org.qsrs.Mock", "-o", "/org/qsrs/Mock", "-m", "org.qsrs.Mock.State"]
        stdout: StdioCollector {
            onStreamFinished: root.st = JSON.parse(text.slice(text.indexOf("'") + 1, text.lastIndexOf("'")))
        }
    }

    PolkitAgent {
        id: agent
        onAuthenticationRequestStarted: root.started++
    }
    property var second: null
    Component { id: secondComponent; PolkitAgent { path: "/org/qsrs/Second" } }
    Connections {
        target: agent.flow
        function onAuthenticationSucceeded() { root.succeeded++ }
        function onAuthenticationFailed() { root.failedSignals++ }
        function onAuthenticationRequestCancelled() { root.daemonCancels++ }
        function onSupplementaryMessageChanged() { root.supplementary.push(agent.flow.supplementaryMessage + (agent.flow.supplementaryIsError ? "!" : "")) }
        function onIsCompletedChanged() {
            const f = agent.flow
            root.ended.push({ cookie: f.cookie, completed: f.isCompleted, success: f.isSuccessful, cancelled: f.isCancelled })
        }
    }

    Timer {
        interval: 50
        repeat: true
        running: true
        onTriggered: {
            root.ticks++
            if (root.ticks > 200) { root.check("timeout at step " + root.step + " registered=" + agent.isRegistered + " state=" + JSON.stringify(root.st), false); root.finish(); return }
            if (!stateProc.running) stateProc.running = true
            const st = root.st
            const flow = agent.flow
            if (!st) return
            switch (root.step) {
            case 0:
                if (!agent.isRegistered || st.registered.length < 1) return
                const reg = st.registered[0]
                root.check("subject unix-session c7", reg.kind === "unix-session" && reg.session === "c7")
                root.check("default path " + reg.path, reg.path === "/org/quickshell/PolkitAgent" && agent.path === reg.path)
                root.check("locale", reg.locale.length > 0)
                root.check("idle", !agent.isActive && flow === null)
                root.second = secondComponent.createObject(root)
                root.mock("Begin", ["c1", "users"])
                root.step = 1
                break
            case 1:
                if (!flow || !flow.isResponseRequired || st.conversations.length < 1) return
                root.check("second agent idle", !root.second.isRegistered && root.second.flow === null && st.registered.length === 1)
                root.check("started signal", root.started === 1 && agent.isActive)
                root.check("strings", flow.message === "Authenticate c1" && flow.actionId === "org.qsrs.test.c1"
                           && flow.iconName === "dialog-password" && flow.cookie === "c1")
                root.check("identities deduped " + flow.identities.length, flow.identities.length === 3)
                root.check("own user first", flow.identities[0].string === st.conversations[0].user && !flow.identities[0].isGroup)
                root.check("root user", flow.identities[1].id === 0 && flow.identities[1].string === "root")
                root.check("group", flow.identities[2].isGroup && flow.identities[2].string === "root" && flow.identities[2].displayName === "root")
                root.check("selected first", flow.selectedIdentity === flow.identities[0])
                root.check("helper got cookie", st.conversations[0].cookie === "c1")
                root.check("prompt", flow.inputPrompt === "Password: " && !flow.responseVisible)
                root.check("info unescaped " + flow.supplementaryMessage, flow.supplementaryMessage.startsWith("Contraseña para ") && !flow.supplementaryIsError)
                root.check("not failed yet", !flow.failed && !flow.isCompleted)
                flow.submit("wrong")
                root.check("submit clears prompt", !flow.isResponseRequired && flow.inputPrompt === "")
                root.step = 2
                break
            case 2:
                if (!flow.failed || !flow.isResponseRequired || st.conversations.length < 2) return
                root.check("failed signal", root.failedSignals === 1)
                root.check("helper got response", st.conversations[0].responses[0] === "wrong")
                root.check("error shown", root.supplementary.includes("Wrong password!"))
                root.check("retry same user", st.conversations[1].user === st.conversations[0].user)
                flow.selectedIdentity = flow.identities[1]
                root.step = 3
                break
            case 3:
                if (st.conversations.length < 3 || !st.conversations[1].closed || !flow.isResponseRequired) return
                root.check("switched to root", st.conversations[2].user === "root")
                root.check("switch is not a failure", root.failedSignals === 1)
                flow.selectedIdentity = null
                root.check("null identity refused", flow.selectedIdentity === flow.identities[1])
                flow.submit("right")
                root.step = 4
                break
            case 4:
                if (root.result("c1") !== "ok" || flow !== null) return
                root.check("succeeded signal", root.succeeded === 1)
                root.check("completed successfully", JSON.stringify(root.ended[0]) === JSON.stringify({ cookie: "c1", completed: true, success: true, cancelled: false }))
                root.mock("Begin", ["c2", "users"])
                root.mock("Begin", ["c3", "users"])
                root.step = 5
                break
            case 5:
                if (!flow || flow.cookie !== "c2" || !flow.isResponseRequired || root.result("c3") !== "pending") return
                root.check("queued request waits", root.started === 2)
                root.mock("Cancel", ["c3"])
                root.step = 6
                break
            case 6:
                if (!root.cancelled("c3")) return
                root.check("queued cancel leaves active", flow && flow.cookie === "c2" && root.started === 2)
                flow.cancelAuthenticationRequest()
                root.step = 7
                break
            case 7:
                if (!root.cancelled("c2") || flow !== null) return
                root.check("user cancel flags", JSON.stringify(root.ended[1]) === JSON.stringify({ cookie: "c2", completed: true, success: false, cancelled: true }))
                root.check("user cancel is not daemon cancel", root.daemonCancels === 0)
                root.mock("Begin", ["c4", "netgroup"])
                root.mock("Begin", ["c5", "users"])
                root.step = 8
                break
            case 8:
                if (!root.cancelled("c4") || !flow || flow.cookie !== "c5" || !flow.isResponseRequired) return
                root.check("no identities reason", root.result("c4").includes("no supported identities"))
                root.mock("Cancel", ["c5"])
                root.step = 9
                break
            case 9:
                if (!root.cancelled("c5") || flow !== null) return
                root.check("daemon cancel signal", root.daemonCancels === 1)
                root.check("daemon cancel flags", root.ended[2].cookie === "c5" && root.ended[2].cancelled && !root.ended[2].success)
                root.check("helper closed", st.conversations[st.conversations.length - 1].closed)
                root.mock("Intrude", [])
                root.step = 10
                break
            case 10:
                if (!st.intruder) return
                root.check("intruder refused " + st.intruder, st.intruder === "org.freedesktop.PolicyKit1.Error.Failed")
                root.check("no flow for intruder", agent.flow === null && root.started === 3)
                root.finish()
            }
        }
    }
}
