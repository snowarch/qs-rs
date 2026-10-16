import QtQml
import Quickshell.Services.UPower

// One UPower device. Values follow quickshell: percentage is 0-1, healthPercentage is the raw
// Capacity (0-100), times are seconds.
QtObject {
    property int type: UPowerDeviceType.Unknown
    property bool powerSupply: false
    property real energy: 0
    property real energyCapacity: 0
    property real changeRate: 0
    property real timeToEmpty: 0
    property real timeToFull: 0
    property real percentage: 0
    property bool isPresent: false
    property int state: UPowerDeviceState.Unknown
    property real healthPercentage: 0
    readonly property bool healthSupported: healthPercentage !== 0
    property string iconName
    readonly property bool isLaptopBattery: type === UPowerDeviceType.Battery && powerSupply
    property string nativePath
    property string model
    property bool ready: false
    property string _path

    function _update(p) {
        if (p.Type !== undefined) type = p.Type
        if (p.PowerSupply !== undefined) powerSupply = p.PowerSupply
        if (p.Energy !== undefined) energy = p.Energy
        if (p.EnergyFull !== undefined) energyCapacity = p.EnergyFull
        if (p.EnergyRate !== undefined) changeRate = p.EnergyRate
        if (p.TimeToEmpty !== undefined) timeToEmpty = p.TimeToEmpty
        if (p.TimeToFull !== undefined) timeToFull = p.TimeToFull
        if (p.Percentage !== undefined) percentage = p.Percentage * 0.01
        if (p.IsPresent !== undefined) isPresent = p.IsPresent
        if (p.State !== undefined) state = p.State
        if (p.Capacity !== undefined) healthPercentage = p.Capacity
        if (p.IconName !== undefined) iconName = p.IconName
        if (p.NativePath !== undefined) nativePath = p.NativePath
        if (p.Model !== undefined) model = p.Model
    }
}
