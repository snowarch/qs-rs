// Quickshell.Networking against the system's NetworkManager (read-only): backend, devices with
// their types and names, a wired device's link, Wi-Fi networks once the scanner runs, enums.
import QtQuick
import Quickshell
import Quickshell.Networking

ShellRoot {
    property var errors: []
    function check(ok, what) { if (!ok) errors.push(what) }
    property int step: 0
    Timer {
        interval: 100; repeat: true; running: true
        onTriggered: {
            step++
            const devs = Networking.devices.values
            if (step < 50 && (Networking.backend !== NetworkBackendType.NetworkManager || devs.length === 0)) return
            if (step === 50 || step > 49) {} // fallthrough after timeout
            check(Networking.backend === NetworkBackendType.NetworkManager, "backend " + Networking.backend)
            check(devs.length > 0, "no devices")
            for (const d of devs) {
                check(d.type === DeviceType.Wifi || d.type === DeviceType.Wired, "type " + d.type)
                check(d.name.length > 0, "device without name")
                console.log("device", d.name, DeviceType.toString(d.type), ConnectionState.toString(d.state),
                            "managed", d.nmManaged, "nets", d.networks.values.map(n => n.name + (n.connected ? "*" : "")).join(","))
                if (d.type === DeviceType.Wired && d.hasLink) check(d.network !== null && d.networks.values.length === 1, "wired link without network")
            }
            check(typeof Networking.wifiEnabled === "boolean", "wifiEnabled")
            check(NetworkConnectivity.toString(NetworkConnectivity.Full) === "Full internet connectivity", "enum toString")
            check(WifiSecurityType.Unknown === 11 && ConnectionState.Disconnected === 4, "enum values")
            console.log("connectivity", NetworkConnectivity.toString(Networking.connectivity), "wifi", Networking.wifiEnabled)
            running = false
            if (errors.length) console.log("FAIL", errors.join("; "))
            Qt.exit(errors.length ? 1 : 0)
        }
    }
}
