import QtQuick
import Quickshell
import Quickshell.Wayland

// Needs a compositor with ext-session-lock-v1 (niri). Never run against the real session:
//   scripts/nested-niri.sh /tmp/x.png 2 qs-rs -p tests/nested/sessionlock.qml   (prints PASS/FAIL)
// The capture at 2s shows the lock surface. Locking makes `locked` true at once and `secure`
// once the compositor confirms; a second lock in the process is refused; unlocking before the
// compositor confirmed (then locking again, or waiting for the confirmation) and unlocking after
// it all leave the session lockable again.
ShellRoot {
    id: root
    property var log: []
    property int step: 0
    property string shown: ""

    function check(name, ok) { if (!ok) log.push("failed: " + name) }

    WlSessionLock {
        id: lock
        WlSessionLockSurface {
            color: "darkred"
            function report() { if (visible) root.shown = width + "x" + height + " " + (screen?.name ?? "none") }
            onVisibleChanged: report()
            onWidthChanged: report()
            onHeightChanged: report()
            Rectangle { anchors.fill: parent; anchors.margins: 40; color: "#203040" }
            Text { anchors.centerIn: parent; text: "locked"; color: "white"; font.pixelSize: 64 }
        }
        onSecureChanged: root.log.push(secure ? "secure" : "insecure")
    }

    WlSessionLock {
        id: other
        WlSessionLockSurface { color: "green" }
    }

    Timer {
        interval: 100
        repeat: true
        running: true
        property int ticks: 0
        onTriggered: {
            ticks++
            if (root.step === 0) {
                // Lock and unlock before the compositor answers: must use destroy, not unlock.
                lock.locked = true
                root.check("locked at once", lock.locked && !lock.secure)
                lock.locked = false
                root.check("unlocked at once", !lock.locked)
                lock.locked = true
                other.locked = true
                root.check("second lock refused", !other.locked)
                root.step = 1
            } else if (root.step === 1 && lock.secure) {
                root.step = 2
                ticks = 0
            } else if (root.step === 2 && ticks === 25) {
                const s = Quickshell.screens[0]
                root.check("surface fills the screen: " + root.shown, root.shown === s.width + "x" + s.height + " " + s.name)
                lock.locked = false
                root.check("unlocked", !lock.locked && !lock.secure)
                lock.locked = true
                root.step = 3
            } else if (root.step === 3 && lock.secure) {
                lock.locked = false
                // Unlocked before the compositor confirms, then left alone: the lock ends by itself.
                lock.locked = true
                lock.locked = false
                root.check("pending unlock reads unlocked", !lock.locked && !lock.secure)
                root.step = 4
                ticks = 0
            } else if (root.step === 4 && ticks === 8) {
                lock.locked = true
                root.step = 5
            } else if (root.step === 5 && lock.secure) {
                lock.locked = false
                root.step = 6
            }
            if (root.step === 6 || ticks > 60) {
                const ok = JSON.stringify(root.log) === '["secure","insecure","secure","insecure","secure","insecure"]'
                console.log(ok ? "PASS" : "FAIL step " + root.step + " " + JSON.stringify(root.log))
                Qt.exit(ok ? 0 : 1)
            }
        }
    }
}
