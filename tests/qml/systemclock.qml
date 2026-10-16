import QtQuick
import Quickshell

// SystemClock: fields truncated to the precision, ticks on second boundaries, enabled toggle.
ShellRoot {
    id: root
    property var errors: []
    property int ticks: 0
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }

    SystemClock { id: secs; precision: SystemClock.Seconds; property int changes: 0; property int worstLag: 0
        onDateChanged: { changes++; worstLag = Math.max(worstLag, Math.abs(new Date() - date)) } }
    SystemClock { id: mins; precision: SystemClock.Minutes }
    SystemClock { id: hrs; precision: SystemClock.Hours }
    SystemClock { id: off; enabled: false; property int changes: 0; onDateChanged: changes++ }

    property int secsStart: -1
    property var offStart: null
    Timer { interval: 100; running: true; onTriggered: { root.secsStart = secs.changes; root.offStart = off.date; secs.worstLag = 0 } }

    Timer {
        interval: 2300
        running: true
        onTriggered: {
            const now = new Date()
            root.check("enum", SystemClock.Hours === 1 && SystemClock.Seconds === 3)
            const ticked = secs.changes - root.secsStart
            root.check("seconds ticked " + ticked, ticked >= 2 && ticked <= 3)
            root.check("ticks on the boundary, lag " + secs.worstLag, secs.worstLag < 150)
            root.check("seconds current", Math.abs(secs.date - now) < 1100 && secs.date.getMilliseconds() === 0)
            root.check("seconds field", secs.seconds === secs.date.getSeconds() && secs.hours === now.getHours())
            root.check("minutes truncated", mins.seconds === 0 && mins.date.getSeconds() === 0 && mins.minutes === mins.date.getMinutes())
            root.check("hours truncated", hrs.minutes === 0 && hrs.seconds === 0 && hrs.hours === hrs.date.getHours())
            root.check("disabled does not tick", off.date.getTime() === root.offStart.getTime())
            if (root.errors.length) console.log("FAILED: " + root.errors.join(", "))
            Qt.exit(root.errors.length ? 1 : 0)
        }
    }
}
