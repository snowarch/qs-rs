pragma Singleton
import QtQml

// spa_audio_channel values.
QtObject {
    enum Enum {
        Unknown = 0, NA = 1, Mono = 2, FrontLeft = 3, FrontRight = 4, FrontCenter = 5,
        LowFrequencyEffects = 6, SideLeft = 7, SideRight = 8, FrontLeftCenter = 9, FrontRightCenter = 10,
        RearCenter = 11, RearLeft = 12, RearRight = 13, TopCenter = 14, TopFrontLeft = 15,
        TopFrontCenter = 16, TopFrontRight = 17, TopRearLeft = 18, TopRearCenter = 19, TopRearRight = 20,
        RearLeftCenter = 21, RearRightCenter = 22, FrontLeftWide = 23, FrontRightWide = 24,
        LowFrequencyEffects2 = 25, FrontLeftHigh = 26, FrontCenterHigh = 27, FrontRightHigh = 28,
        TopFrontLeftCenter = 29, TopFrontRightCenter = 30, TopSideLeft = 31, TopSideRight = 32,
        LowFrequencyEffectsLeft = 33, LowFrequencyEffectsRight = 34, BottomCenter = 35,
        BottomLeftCenter = 36, BottomRightCenter = 37,
        AuxRangeStart = 4096, AuxRangeEnd = 8191, CustomRangeStart = 65536
    }

    readonly property var _names: ["Unknown", "N/A", "Mono", "Front Left", "Front Right", "Front Center",
        "Low Frequency Effects", "Side Left", "Side Right", "Front Left Center", "Front Right Center",
        "Rear Center", "Rear Left", "Rear Right", "Top Center", "Top Front Left", "Top Front Center",
        "Top Front Right", "Top Rear Left", "Top Rear Center", "Top Rear Right", "Rear Left Center",
        "Rear Right Center", "Front Left Wide", "Front Right Wide", "Low Frequency Effects 2",
        "Front Left High", "Front Center High", "Front Right High", "Top Front Left Center",
        "Top Front Right Center", "Top Side Left", "Top Side Right", "Low Frequency Effects Left",
        "Low Frequency Effects Right", "Bottom Center", "Bottom Left Center", "Bottom Right Center"]

    function toString(channel) {
        if (channel >= 0 && channel < _names.length) return _names[channel]
        if (channel >= 4096 && channel <= 8191) return "Aux " + (channel - 4096)
        if (channel >= 65536) return "Custom " + (channel - 65536)
        return "Unknown"
    }
}
