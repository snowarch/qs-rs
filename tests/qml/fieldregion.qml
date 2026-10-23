import QtQuick
import Quickshell

// FieldRegion holds a pixel when its centre lies half a pixel inside the field inir's IrisField.frag draws:
// straight edges, the frame band, a body's fillet into the band, a squircle corner and a chamfer cut.
ShellRoot {
    id: root
    FieldRegion {
        id: plain
        windowWidth: 400; windowHeight: 300
        bodies: [10, 10, 100, 100, 0, 0, 0, 0]
    }
    FieldRegion {
        id: framed
        windowWidth: 400; windowHeight: 300
        framed: true; band: 12; cornerRadius: 20
        // Hangs from the top band with a 20 px fillet; its corners square, so only the fillet curves.
        bodies: [150, 0, 100, 60, 0, 20, -1, 0]
    }
    FieldRegion {
        id: far
        windowWidth: 800; windowHeight: 600
        framed: true; band: 12; cornerRadius: 20
        // A body deep inside the opening: a trace that stepped by the band's distance alone jumped into it.
        bodies: [100, 300, 500, 200, 20, 0, 0, 0]
    }
    FieldRegion {
        id: round
        windowWidth: 400; windowHeight: 300
        bodies: [100, 100, 200, 120, 30, 0, 0, 0]
    }
    FieldRegion {
        id: cut
        windowWidth: 400; windowHeight: 300
        chamfer: 10
        bodies: [100, 100, 100, 100, 0, 0, 0, 0]
    }
    FieldRegion {
        id: wave
        windowWidth: 400; windowHeight: 300
        framed: true; band: 12; cornerRadius: 20
        // The music swells every side; straight rows are read off the distance, corner rows are traced.
        wave: Qt.vector4d(20, 15, 18, 10); waveClock: 1.3
    }
    FieldRegion {
        id: sides
        windowWidth: 400; windowHeight: 300
        framed: true; band: 12; cornerRadius: 20
        // Only the sides swell, so straight stretches of a row are answered by one distance: a body hung from the top,
        // one floating, one welded to the bottom near a corner.
        wave: Qt.vector4d(20, 0, 18, 0); waveClock: 0.7
        bodies: [150, 0, 100, 60, 14, 20, -1, 0, 40, 200, 80, 40, 10, 0, 0, 0, 250, 250, 120, 50, 16, 12, -1, 0]
    }
    function smin(a, b, k) {
        if (k <= 0.001) return Math.min(a, b)
        const h = Math.max(0, Math.min(1, 0.5 + 0.5 * (b - a) / k))
        return b + (a - b) * h - k * h * (1 - h)
    }
    function sidesHolds(x, y) {
        const px = x + 0.5, py = y + 0.5, t = 0.7
        const ay = 0.60 + 0.24 * Math.sin(py * 0.011 + t) + 0.16 * Math.sin(py * 0.027 - t * 1.4)
        const lx = 12 + 20 * ay, hx = 400 - 12 - 18 * ay, ly = 12, hy = 288
        const frame = -root.squircle(px, py, (lx + hx) / 2, (ly + hy) / 2, (hx - lx) / 2, (hy - ly) / 2, 20)
        let d = frame
        const b = sides.bodies
        for (let n = 0; n < b.length; n += 8) {
            const body = root.squircle(px, py, b[n] + b[n + 2] / 2, b[n + 1] + b[n + 3] / 2, b[n + 2] / 2, b[n + 3] / 2, b[n + 4])
            d = Math.min(d, body)
            if (b[n + 6] === -1) d = Math.min(d, root.smin(frame, body, b[n + 5]))
        }
        return d + 0.5 < 0
    }
    // IrisField.frag's band with its swell, in JS: the truth both paths must give.
    function squircle(px, py, cx, cy, hx, hy, r) {
        const reach = Math.min(r * 1.8409, Math.min(hx, hy))
        const qx = Math.abs(px - cx) - (hx - reach), qy = Math.abs(py - cy) - (hy - reach)
        if (qx <= 0 || qy <= 0) return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - reach
        const x2 = qx * qx, y2 = qy * qy, l = Math.sqrt(Math.sqrt(x2 * x2 + y2 * y2)), l3 = l * l * l
        return (l - reach) / Math.hypot(x2 * qx / l3, y2 * qy / l3)
    }
    function waveHolds(x, y) {
        const px = x + 0.5, py = y + 0.5, t = 1.3
        const ay = 0.60 + 0.24 * Math.sin(py * 0.011 + t) + 0.16 * Math.sin(py * 0.027 - t * 1.4)
        const ax = 0.60 + 0.24 * Math.sin(px * 0.009 - t) + 0.16 * Math.sin(px * 0.023 + t * 1.2)
        const lx = 12 + 20 * ay, ly = 12 + 15 * ax, hx = 400 - 12 - 18 * ay, hy = 300 - 12 - 10 * ax
        return -squircle(px, py, (lx + hx) / 2, (ly + hy) / 2, (hx - lx) / 2, (hy - ly) / 2, 20) + 0.5 < 0
    }
    // Every pixel of a row against the JS truth: corner rows inside the swell, straight rows between them.
    function waveChecks() {
        const out = []
        for (const y of [20, 45, 70, 110, 150, 190, 230, 262, 285]) {
            const got = wave.row(y)
            for (let x = 0; x < 400; ++x) out.push([got[x] === "1", root.waveHolds(x, y)])
        }
        for (let y = 0; y < 300; y += 3) {
            const got = sides.row(y)
            for (let x = 0; x < 400; ++x) out.push([got[x] === "1", root.sidesHolds(x, y)])
        }
        return out
    }
    Component.onCompleted: {
        const checks = [
            [plain.holds(10, 50), false], [plain.holds(11, 50), true], [plain.holds(108, 50), true], [plain.holds(109, 50), false],
            [plain.holds(50, 10), false], [plain.holds(50, 11), true], [plain.holds(200, 200), false],
            // The band: pixel 10 is 1.5 inside it, pixel 11 sits on the half-pixel line.
            [framed.holds(5, 150), true], [framed.holds(10, 150), true], [framed.holds(11, 150), false], [framed.holds(200, 150), false],
            // Beside the body, just under the band, the fillet fills what a plain union leaves open.
            [framed.holds(147, 13), true], [framed.holds(130, 20), false], [framed.holds(200, 40), true],
            // A squircle corner is held further in than the box corner, but its sides are straight.
            [round.holds(101, 101), false], [round.holds(108, 108), false], [round.holds(101, 160), true], [round.holds(150, 101), true],
            [far.holds(102, 400), true], [far.holds(130, 450), true], [far.holds(350, 320), true], [far.holds(98, 400), false],
            // A chamfered Design cuts the upper-right and lower-left corners only.
            [cut.holds(101, 101), true], [cut.holds(198, 101), false], [cut.holds(101, 198), false], [cut.holds(198, 198), true],
        ].concat(root.waveChecks())
        const bad = checks.map((c, i) => c[0] === c[1] ? -1 : i).filter(i => i >= 0)
        if (bad.length) { console.log("fieldregion: failed checks", bad.join(","), JSON.stringify(framed.stats())); Qt.exit(1) }
        else { console.log("fieldregion: ok", JSON.stringify(framed.stats())); Qt.exit(0) }
    }
}
