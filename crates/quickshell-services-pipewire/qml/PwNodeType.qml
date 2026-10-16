pragma Singleton
import QtQml

// Node type flags, same values as quickshell's PwNodeType.
QtObject {
    enum Flags {
        Untracked = 0,
        Audio = 1,
        Video = 2,
        Stream = 4,
        Source = 8,
        Sink = 16,
        AudioSink = 17,
        AudioSource = 9,
        AudioDuplex = 25,
        AudioOutStream = 21,
        AudioInStream = 13,
        VideoSource = 10,
        VideoSink = 18
    }

    readonly property var _byClass: ({
        "Audio/Sink": 17, "Audio/Source": 9, "Audio/Duplex": 25,
        "Stream/Output/Audio": 21, "Stream/Input/Audio": 13,
        "Video/Sink": 18, "Video/Source": 10
    })
    function _fromMediaClass(mediaClass) { return _byClass[mediaClass] ?? 0 }

    function toString(type) {
        const names = { 0: "Untracked", 17: "AudioSink", 9: "AudioSource", 25: "AudioDuplex", 21: "AudioOutStream",
                        13: "AudioInStream", 10: "VideoSource", 18: "VideoSink" }
        if (names[type] !== undefined) return names[type]
        const parts = []
        for (const [bit, name] of [[1, "Audio"], [2, "Video"], [4, "Stream"], [8, "Source"], [16, "Sink"]])
            if (type & bit) parts.push(name)
        return parts.join(" | ")
    }
}
