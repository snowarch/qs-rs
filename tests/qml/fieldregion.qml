import QtQuick
import Quickshell

// FieldRegion holds a pixel when its centre lies half a pixel inside the field inir's IrisField.frag draws:
// straight edges, the frame band, a body's fillet into the band, a squircle corner and a chamfer cut.
ShellRoot {
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
        ]
        const bad = checks.map((c, i) => c[0] === c[1] ? -1 : i).filter(i => i >= 0)
        if (bad.length) { console.log("fieldregion: failed checks", bad.join(","), JSON.stringify(framed.stats())); Qt.exit(1) }
        else { console.log("fieldregion: ok", JSON.stringify(framed.stats())); Qt.exit(0) }
    }
}
