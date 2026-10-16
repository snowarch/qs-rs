import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Mpris

// Runs against tests/mpris/mock.py: player discovery, metadata, capabilities, the computed
// position, play, setting volume and position, next track, and removal when the player quits.
ShellRoot {
    id: root
    property var errors: []
    property int step: 0
    property int ticks: 0
    property int tracks: 0
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }
    readonly property MprisPlayer player: Mpris.players.values[0] ?? null
    Connections { target: root.player; function onTrackChanged() { root.tracks++ } }

    Process {
        id: calls
        property string out: ""
        command: ["gdbus", "call", "--session", "-d", "org.qsrs.Mock", "-o", "/org/qsrs/Mock", "-m", "org.qsrs.Mock.Calls"]
        stdout: StdioCollector { onStreamFinished: calls.out = text }
    }

    Timer {
        interval: 50
        repeat: true
        running: true
        onTriggered: {
            root.ticks++
            if (root.ticks > 160) { root.check("timeout at step " + root.step, false); root.finish(); return }
            const p = root.player
            switch (root.step) {
            case 0:
                if (!p || !p.positionSupported) return
                root.check("enums", MprisPlaybackState.Paused === 2 && MprisLoopState.Playlist === 2)
                root.check("toString", MprisPlaybackState.toString(MprisPlaybackState.Playing) === "Playing")
                root.check("identity", p.identity === "Mock Player" && p.desktopEntry === "mock" && p.dbusName === "org.mpris.MediaPlayer2.qsrs")
                root.check("title", p.trackTitle === "Song 1" && p.trackArtist === "A, B" && p.trackAlbum === "Album")
                root.check("art", p.trackArtUrl === "file:///art.png")
                root.check("length", p.lengthSupported && p.length === 200)
                root.check("caps", p.canPlay && p.canPause && p.canSeek && p.canGoNext && !p.canGoPrevious && p.canQuit && !p.canRaise)
                root.check("toggle", p.canTogglePlaying && !p.isPlaying && p.playbackState === MprisPlaybackState.Paused)
                root.check("paused position", Math.abs(p.position - 10) < 0.05)
                root.check("volume", p.volume === 0.5 && p.volumeSupported && p.loopSupported && p.shuffleSupported)
                root.check("rate range", p.minRate === 0.5 && p.maxRate === 2)
                p.play()
                root.step = 1
                break
            case 1:
                if (!p.isPlaying) return
                root.step = 2
                root.ticks = 100
                break
            case 2:
                if (root.ticks < 120) return
                p.positionChanged()
                root.check("position advances " + p.position.toFixed(2), p.position > 10.7 && p.position < 11.5)
                p.volume = 0.25
                p.position = 50
                root.step = 3
                break
            case 3:
                if (p.volume !== 0.25) return
                p.positionChanged()
                root.check("seeked " + p.position.toFixed(2), p.position >= 50 && p.position < 50.5)
                p.next()
                root.step = 4
                break
            case 4:
                if (p.trackTitle !== "Song 2") return
                root.check("unique id " + p.uniqueId + " " + root.tracks, p.uniqueId === 2 && root.tracks === 1)
                calls.running = true
                root.step = 5
                break
            case 5:
                if (!calls.out) return
                root.check("calls " + calls.out.trim(), calls.out.includes("Play;Set Volume=0.25;SetPosition /track/1 50000000;Next"))
                p.quit()
                root.step = 6
                break
            case 6:
                if (Mpris.players.values.length !== 0) return
                root.finish()
            }
        }
    }
}
