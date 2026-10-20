import QtQuick
import Quickshell

// SpringValue against what IrisSpring promises: settles exactly on `to`, bounces only with
// bounce, keeps its velocity on a retarget, runs a curve in its own time and adds a second one
// on top, jump() and animate: false land at once, minimum holds, and recede is used going down.
ShellRoot {
    id: root
    property var samples: []
    property double kickedAt: 0
    property int stage: -1
    property var failures: []

    function check(ok, what) {
        if (!ok) failures.push(what)
        console.log((ok ? "ok   " : "FAIL ") + what)
    }

    SpringValue {
        id: s
        emerge: ({ response: 300, bounce: 0 })
        onValueChanged: root.samples.push({ t: Date.now() - root.kickedAt, v: value, vel: velocity })
        onRunningChanged: if (!running && root.stage >= 0) root.settled()
    }

    function go(to) {
        samples = []
        kickedAt = Date.now()
        s.to = to
    }

    property var stages: [
        function() { s.emerge = { response: 300, bounce: 0 }; go(1) },
        function() { s.emerge = { response: 300, bounce: 0.4 }; s.recede = { response: 900, bounce: 0 }; go(0) },
        function() { go(1) },
        function() { s.emerge = { response: 400, bounce: 0 }; s.recede = null; s.animate = false; s.to = 0; s.animate = true; go(1); later(120, function() { root.retargetAt = s.value; root.retargetN = root.samples.length; s.to = 0 }) },
        function() { s.emerge = { response: 200, bounce: 0, curve: [0.3, 0, 0.2, 1] }; s.recede = null; go(1) },
        function() { go(2); later(90, function() { s.to = 3 }) },
        function() { s.emerge = { response: 500, bounce: 0 }; go(0); later(60, function() { s.jump() }) },
        function() { s.minimum = 0; s.emerge = { response: 300, bounce: 0.7 }; s.recede = { response: 300, bounce: 0.7 }; go(1); }
    ]
    property real retargetAt: 0
    property int retargetN: 0

    function later(ms, f) {
        const t = Qt.createQmlObject("import QtQuick; Timer { }", root)
        t.interval = ms
        t.triggered.connect(function() { f(); t.destroy() })
        t.start()
    }

    function maxOf(list) { return list.reduce((m, x) => Math.max(m, x.v), -1e9) }
    function minOf(list) { return list.reduce((m, x) => Math.min(m, x.v), 1e9) }

    function settled() {
        const last = samples.length ? samples[samples.length - 1] : { t: 0, v: s.value }
        const n = stage
        if (n === 0) {
            check(s.value === 1 && s.velocity === 0, "settles exactly on to (" + s.value + ")")
            check(samples.every((x, i) => i === 0 || x.v >= samples[i - 1].v), "no bounce at bounce 0")
            check(last.t > 150 && last.t < 900, "300 ms spring settles in " + last.t + " ms")
        } else if (n === 1) {
            check(last.t > 700, "going down uses recede (900 ms spring settles in " + last.t + " ms)")
        } else if (n === 2) {
            check(maxOf(samples) > 1.05, "bounce 0.4 overshoots (" + maxOf(samples).toFixed(3) + ")")
            check(last.t < 900, "an overshoot stays on emerge (settles in " + last.t + " ms)")
        } else if (n === 3) {
            const after = samples[root.retargetN]
            check(after && after.v > root.retargetAt, "retarget keeps the velocity (" + root.retargetAt.toFixed(3) + " -> " + (after ? after.v.toFixed(3) : "none") + ")")
            check(s.value === 0, "retarget lands on the new to")
        } else if (n === 4) {
            check(s.value === 1, "curve lands on to")
            check(last.t >= 180 && last.t < 300, "200 ms curve ends at " + last.t + " ms")
        } else if (n === 5) {
            check(s.value === 3, "added curve lands on the last to")
            let jump = 0
            for (let i = 1; i < samples.length; i++) jump = Math.max(jump, Math.abs(samples[i].v - samples[i - 1].v))
            check(jump < 0.5, "two curves add without a jump (largest step " + jump.toFixed(3) + ")")
        } else if (n === 6) {
            check(s.value === 0 && s.velocity === 0 && last.t < 200, "jump() lands at once")
        } else if (n === 7) {
            go(0)
            root.stage = 8
            return
        } else if (n === 8) {
            check(minOf(samples) >= 0, "minimum holds (" + minOf(samples).toFixed(3) + ")")
            s.animate = false
            s.to = 5
            check(s.value === 5 && !s.running, "animate: false lands at once")
            check(Quickshell.hasVersion(0, 3, ["native-spring"]), "native-spring feature")
            Qt.exit(failures.length === 0 ? 0 : 1)
            return
        }
        next()
    }

    function next() {
        stage++
        later(30, function() { root.stages[root.stage]() })
    }

    Component.onCompleted: next()
    Timer { interval: 15000; running: true; onTriggered: { console.log("FAIL timeout at stage " + root.stage); Qt.exit(1) } }
}
