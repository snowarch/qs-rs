import QtQuick
import Quickshell

// QSRS_SLOW=4 (wrap.sh), as a nest filming motion sets it: the driver's clock goes at a quarter of the wall clock, so
// a 200 ms animation and a 200 ms Timer (an animation job too) both take about 800 ms. Without it they take 200.
ShellRoot {
    id: root
    property real start: Date.now()
    property real value: 0
    property real animated: -1
    property real timed: -1

    function done() {
        if (root.animated < 0 || root.timed < 0) return
        const ok = root.animated > 650 && root.animated < 1400 && root.timed > 650 && root.timed < 1400
        if (!ok) console.log("check failed: animation " + root.animated + " ms, timer " + root.timed + " ms")
        Qt.exit(ok ? 0 : 1)
    }

    NumberAnimation on value {
        from: 0
        to: 1
        duration: 200
        running: true
        onFinished: {
            root.animated = Date.now() - root.start
            root.done()
        }
    }
    Timer {
        interval: 200
        running: true
        onTriggered: {
            root.timed = Date.now() - root.start
            root.done()
        }
    }
}
