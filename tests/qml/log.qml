import QtQuick
import Quickshell
import Quickshell.Io

// Instance log: QML messages reach the detailed log of this instance, and `log` (the upstream
// `qs log` command) decodes it from the live instance, filtered by -r rules.
ShellRoot {
    id: root
    function finish(ok, why) { console.log(ok ? "PASS" : "FAIL " + why); Qt.exit(ok ? 0 : 1) }
    Process {
        id: reader
        // The shell's own binary, found through the parent of sh.
        command: ["sh", "-c", 'exec /proc/$PPID/exe -p "$1" log --no-color -r "qml.info=false"', "sh", Quickshell.shellPath("log.qml")]
        stdout: StdioCollector { id: out }
        onExited: code => {
            const t = out.text;
            if (code !== 0) root.finish(false, "exit " + code);
            else if (!t.includes("  WARN qml: log-marker-warn")) root.finish(false, "no warning in " + JSON.stringify(t));
            else if (t.includes("log-marker-info")) root.finish(false, "rule not applied");
            else if (!t.includes("  INFO: Configuration Loaded")) root.finish(false, "no load line");
            else root.finish(true);
        }
    }
    Timer {
        interval: 200; running: true
        onTriggered: { console.warn("log-marker-warn"); console.info("log-marker-info"); reader.running = true }
    }
}
