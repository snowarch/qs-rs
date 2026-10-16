import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.UPower

// Runs against tests/upower/mock.py on a private system bus: device properties and scaling,
// property changes, devices added and removed, power profiles read, write and holds.
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
    function near(a, b) { return Math.abs(a - b) < 0.0001 }
    function device(model) { return UPower.devices.values.find(d => d.model === model) ?? null }

    Process {
        id: mock
        property int n: 0
        command: ["gdbus", "call", "--system", "-d", "org.qsrs.Mock", "-o", "/org/qsrs/Mock", "-m", "org.qsrs.Mock.Step", String(n)]
    }
    Process {
        id: active
        property string out: ""
        command: ["gdbus", "call", "--system", "-d", "org.freedesktop.UPower.PowerProfiles", "-o", "/org/freedesktop/UPower/PowerProfiles",
                  "-m", "org.freedesktop.DBus.Properties.Get", "org.freedesktop.UPower.PowerProfiles", "ActiveProfile"]
        stdout: StdioCollector { onStreamFinished: active.out = text }
    }
    function mockStep(n) { mock.n = n; mock.running = true }

    Timer {
        interval: 50
        repeat: true
        running: true
        onTriggered: {
            root.ticks++
            if (root.ticks > 120) { root.check("timeout at step " + root.step, false); root.finish(); return }
            const bat = root.device("Test Battery")
            switch (root.step) {
            case 0:
                if (!bat || !UPower.displayDevice.ready || PowerProfiles.holds.length === 0) return
                root.check("enums", UPowerDeviceState.Charging === 1 && UPowerDeviceType.LinePower === 1 && PowerProfile.Performance === 2)
                root.check("type toString", UPowerDeviceType.toString(UPowerDeviceType.BluetoothGeneric) === "Bluetooth Generic")
                root.check("state toString", UPowerDeviceState.toString(UPowerDeviceState.FullyCharged) === "Fully Charged")
                root.check("one device", UPower.devices.values.length === 1)
                root.check("percentage scaled", root.near(bat.percentage, 0.75))
                root.check("display device", root.near(UPower.displayDevice.percentage, 0.75) && UPower.displayDevice.isLaptopBattery)
                root.check("type", bat.type === UPowerDeviceType.Battery)
                root.check("state", bat.state === UPowerDeviceState.Charging)
                root.check("laptop battery", bat.isLaptopBattery)
                root.check("health", bat.healthPercentage === 91 && bat.healthSupported)
                root.check("energy", bat.energy === 30 && bat.energyCapacity === 50 && bat.changeRate === 7.5)
                root.check("times", bat.timeToEmpty === 3600 && bat.timeToFull === 0)
                root.check("strings", bat.iconName === "battery-good-symbolic" && bat.nativePath === "BAT0")
                root.check("ready", bat.ready && bat.isPresent)
                root.check("not on battery", UPower.onBattery === false)
                root.check("profile", PowerProfiles.profile === PowerProfile.Balanced)
                root.check("has performance", PowerProfiles.hasPerformanceProfile)
                root.check("no degradation", PowerProfiles.degradationReason === PerformanceDegradationReason.None)
                const h = PowerProfiles.holds[0]
                root.check("hold", h.profile === PowerProfile.PowerSaver && h.applicationId === "test" && h.reason === "testing")
                root.mockStep(1)
                root.step = 1
                break
            case 1:
                if (!UPower.onBattery || bat.state !== UPowerDeviceState.Discharging) return
                root.check("percentage changed", root.near(bat.percentage, 0.4))
                root.mockStep(2)
                root.step = 2
                break
            case 2:
                if (!root.device("Mouse")) return
                root.check("mouse type", root.device("Mouse").type === UPowerDeviceType.Mouse)
                root.check("mouse not laptop battery", !root.device("Mouse").isLaptopBattery)
                root.mockStep(3)
                root.step = 3
                break
            case 3:
                if (bat) return
                root.check("battery removed", UPower.devices.values.length === 1)
                PowerProfiles.profile = PowerProfile.PowerSaver
                root.mockStep(4)
                root.step = 4
                break
            case 4:
                if (PowerProfiles.degradationReason !== PerformanceDegradationReason.HighTemperature) return
                active.running = true
                root.step = 5
                break
            case 5:
                // The daemon itself must hold the written profile.
                if (!active.out) return
                root.check("daemon profile " + active.out.trim(), active.out.includes("power-saver"))
                root.check("profile kept", PowerProfiles.profile === PowerProfile.PowerSaver)
                root.finish()
            }
        }
    }
}
