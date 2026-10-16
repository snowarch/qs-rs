import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Pipewire

// Runs against the private daemon of tests/pipewire/wrap.sh: nodes, defaults, enums, volume and
// mute round trips (checked with pw-dump, a separate client), links and the link tracker.
ShellRoot {
    id: root
    property var errors: []
    property int step: 0
    property int ticks: 0
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }
    function near(a, b) { return Math.abs(a - b) < 0.01 }

    readonly property PwNode sink: Pipewire.defaultAudioSink
    readonly property PwNode source: Pipewire.nodes.values.find(n => n.name === "qsrs-test-source") ?? null

    PwObjectTracker { objects: [root.sink] }
    PwNodeLinkTracker { id: tracker; node: root.sink }

    // Props of a node as seen by another client: [volumes (cubic root), mute].
    property var dumped: null
    Process {
        id: dump
        property int node: -1
        command: ["pw-dump", String(node)]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    const props = JSON.parse(text)[0].info.params.Props.find(p => p.channelVolumes !== undefined)
                    root.dumped = [props.channelVolumes.map(v => Math.cbrt(v)), props.mute]
                } catch (e) {
                    root.dumped = ["error", String(e)]
                }
            }
        }
    }
    Process { id: link; command: ["pw-link", "qsrs-test-source:capture_FR", "qsrs-test-sink:playback_FR"] }

    Timer {
        interval: 50
        repeat: true
        running: true
        onTriggered: {
            root.ticks++
            if (root.ticks > 120) { root.check("timeout at step " + root.step, false); root.finish(); return }
            switch (root.step) {
            case 0:
                if (!Pipewire.ready || !root.sink || !root.sink.ready || !root.source) return
                root.check("enum AudioSink", PwNodeType.AudioSink === 17)
                root.check("enum Active", PwLinkState.Active === 4 && PwLinkState.Error === -2)
                root.check("enum FrontLeft", PwAudioChannel.FrontLeft === 3)
                root.check("channel toString", PwAudioChannel.toString(PwAudioChannel.FrontRight) === "Front Right")
                root.check("sink name", root.sink.name === "qsrs-test-sink")
                root.check("sink description", root.sink.description === "QsRs Test Sink")
                root.check("sink nickname", root.sink.nickname === "Test Sink")
                root.check("sink type", root.sink.type === PwNodeType.AudioSink)
                root.check("sink isSink", root.sink.isSink && !root.sink.isStream)
                root.check("source not sink", !root.source.isSink && root.source.type === PwNodeType.AudioSource)
                root.check("properties", root.sink.properties["media.class"] === "Audio/Sink")
                root.check("channels", String(root.sink.audio.channels) === "3,4")
                root.check("initial volume", root.near(root.sink.audio.volume, 1))
                root.check("not muted", root.sink.audio.muted === false)
                root.check("no default source", Pipewire.defaultAudioSource === null)
                root.sink.audio.volume = 0.5
                root.step = 1
                break
            case 1:
                if (!root.near(root.sink.audio.volumes[0] ?? 0, 0.5)) return
                root.check("both channels", root.near(root.sink.audio.volumes[1], 0.5))
                root.sink.audio.muted = true
                dump.node = root.sink.id
                root.dumped = null
                dump.running = true
                root.step = 2
                break
            case 2:
                if (root.dumped === null) return
                root.check("pw-dump volume " + JSON.stringify(root.dumped),
                           root.near(root.dumped[0][0], 0.5) && root.near(root.dumped[0][1], 0.5))
                if (root.dumped[1] !== true) { root.dumped = null; dump.running = true; return }
                root.sink.audio.volumes = [0.2, 0.4]
                root.step = 3
                break
            case 3:
                if (!root.near(root.sink.audio.volume, 0.3)) return
                root.check("volumes kept balance", root.near(root.sink.audio.volumes[0], 0.2))
                root.sink.audio.muted = false
                Pipewire.preferredDefaultAudioSink = root.source
                link.running = true
                root.step = 4
                break
            case 4:
                if (tracker.linkGroups.length === 0 || Pipewire.links.values.length === 0) return
                if (Pipewire.preferredDefaultAudioSink !== root.source) return
                const g = tracker.linkGroups[0]
                root.check("group source", g.source === root.source)
                root.check("group target", g.target === root.sink)
                root.check("link ends", Pipewire.links.values[0].source === root.source)
                root.check("link state", PwLinkState.toString(g.state) !== "Invalid")
                root.check("unmuted", root.sink.audio.muted === false)
                root.finish()
            }
        }
    }
}
