import QtQuick
import Quickshell
import Quickshell.Wayland

// Needs a compositor with ext-idle-notify-v1 and no input:
//   scripts/nested.sh /tmp/x.png 4 qs-rs -p tests/nested/idlemonitor.qml   (prints PASS/FAIL)
// Idle after the timeout, a timeout change restarts it, a disabled monitor never fires.
ShellRoot {
    id: root
    property var seen: []
    IdleMonitor { id: m; timeout: 1; respectInhibitors: false; onIsIdleChanged: root.seen.push(isIdle) }
    IdleMonitor { id: off; enabled: false; timeout: 0.3; onIsIdleChanged: root.seen.push("off") }
    Timer { interval: 1600; running: true; onTriggered: { root.seen.push("retime"); m.timeout = 0.5 } }
    Timer {
        interval: 2600
        running: true
        onTriggered: {
            const ok = JSON.stringify(root.seen) === '[true,"retime",false,true]'
            console.log(ok ? "PASS" : "FAIL " + JSON.stringify(root.seen))
            Qt.exit(ok ? 0 : 1)
        }
    }
}
