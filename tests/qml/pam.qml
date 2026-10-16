import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Pam

// PamContext against test PAM stacks: pam_permit / pam_deny in ./pam (relative configDirectory),
// and pam_exec expose_authtok (prompts for a password, checked by pam/check.sh) for the full
// conversation, wrong answers, abort, and the setters' active guard.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    property var results: []
    property string runDir: Quickshell.env("XDG_RUNTIME_DIR") + "/pamtest"
    property var steps: []
    function next() {
        const s = steps.shift()
        if (s) s()
        else { console.log(errors.length ? "FAIL " + errors.join(", ") : "PASS"); Qt.exit(errors.length ? 1 : 0) }
    }

    // Declared active: starts on componentComplete.
    PamContext {
        id: early
        active: true
        configDirectory: "pam"
        config: "permit"
        onCompleted: result => root.results.push("early " + result)
    }

    PamContext {
        id: pam
        configDirectory: "pam"
        property string answer
        property bool abortOnPrompt: false
        onCompleted: result => { root.results.push(config + " " + result); Qt.callLater(root.next) }
        onError: error => root.results.push("error " + error)
        onPamMessage: {
            root.results.push(`msg ${message.trim()} req=${responseRequired} vis=${responseVisible} err=${messageIsError}`)
            if (!responseRequired) return
            if (abortOnPrompt) {
                config = "deny"
                root.check("config locked while active", config === "password")
                abort()
                root.check("abort clears", !active && message === "" && !responseRequired)
                Qt.callLater(root.next)
            } else respond(answer)
        }
    }

    Process {
        id: writer
        // Like pam_unix: a wrong password ends in pam_deny (PAM_AUTH_ERR), not a module error.
        command: ["sh", "-c", 'mkdir -p "$1" && printf "auth [success=1 default=ignore] pam_exec.so expose_authtok quiet %s\\nauth requisite pam_deny.so\\nauth required pam_permit.so\\n" "$2" > "$1/password"',
                  "sh", root.runDir, Qt.resolvedUrl("pam/check.sh").toString().replace("file://", "")]
        onExited: root.next()
    }

    Timer { id: settle; interval: 500; onTriggered: root.next() }
    Timer { interval: 8000; running: true; onTriggered: { root.check("timeout at " + root.results.join(" | "), false); root.next() } }

    Component.onCompleted: {
        steps = [
            () => {
                check("early completed " + results.join("|"), results[0] === "early " + PamResult.Success && !early.active)
                results = []
                pam.config = "permit"
                check("start returns true", pam.start() && pam.active)
            },
            () => {
                check("permit " + results, results.join("|") === "permit " + PamResult.Success && !pam.active)
                results = []
                pam.config = "deny"
                pam.start()
            },
            () => {
                check("deny " + results, results.join("|") === "deny " + PamResult.Failed)
                results = []
                pam.config = "missing"
                check("missing config does not start", !pam.start() && !pam.active)
                writer.running = true
            },
            () => {
                pam.configDirectory = runDir
                pam.config = "password"
                pam.answer = "hunter2"
                pam.start()
            },
            () => {
                check("password " + results.join("|"), results.join("|") === `msg Password: req=true vis=false err=false|password ${PamResult.Success}`)
                results = []
                pam.answer = "wrong"
                pam.start()
            },
            () => {
                check("wrong password " + results.join("|"), results.join("|") === `msg Password: req=true vis=false err=false|password ${PamResult.Failed}`)
                results = []
                pam.abortOnPrompt = true
                pam.start()
            },
            () => settle.start(),
            () => {
                check("no completion after abort " + results.join("|"), results.length === 1)
                pam.abortOnPrompt = false
                pam.config = "permit"
                pam.configDirectory = "pam"
                pam.active = true
            },
            () => {
                check("restart after abort " + results.join("|"), results[1] === "permit " + PamResult.Success)
                next()
            },
        ]
        // `early` completes asynchronously too.
        settle.start()
    }
}
