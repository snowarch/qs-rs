pragma Singleton
import QtQml
import Quickshell.Services.UPower

// power-profiles-daemon state. Writing `profile` asks the daemon; the value it confirms comes
// back through PropertiesChanged.
QtObject {
    id: root
    property int profile: PowerProfile.Balanced
    property bool hasPerformanceProfile: false
    property int degradationReason: PerformanceDegradationReason.None
    property var holds: []
    property bool _syncing: false

    onProfileChanged: {
        if (_syncing) return
        if (profile === PowerProfile.Performance && !hasPerformanceProfile) {
            console.warn("PowerProfiles: cannot request the performance profile, it is not present on this device")
            return
        }
        PowerProfilesCore.setActiveProfile(PowerProfile._wire[profile] ?? "balanced")
    }

    function _profile(wire) { return Math.max(0, PowerProfile._wire.indexOf(wire)) }

    property Connections _core: Connections {
        target: PowerProfilesCore
        function onPropertiesChanged(json) {
            const p = JSON.parse(json)
            root._syncing = true
            if (p.ActiveProfile !== undefined) root.profile = root._profile(p.ActiveProfile)
            if (p.Profiles !== undefined) root.hasPerformanceProfile = p.Profiles.some(e => e.Profile === "performance")
            if (p.PerformanceDegraded !== undefined)
                root.degradationReason = ({ "lap-detected": PerformanceDegradationReason.LapDetected,
                                            "high-operating-temperature": PerformanceDegradationReason.HighTemperature })[p.PerformanceDegraded]
                                         ?? PerformanceDegradationReason.None
            if (p.ActiveProfileHolds !== undefined)
                root.holds = p.ActiveProfileHolds.map(h => ({ profile: root._profile(h.Profile), applicationId: h.ApplicationId ?? "", reason: h.Reason ?? "" }))
            root._syncing = false
        }
    }
}
