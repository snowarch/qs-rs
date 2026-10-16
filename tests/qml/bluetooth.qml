import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Bluetooth

// Runs against tests/bluetooth/mock.py: adapter and devices from the object manager, battery,
// property writes, discovery, connect with its Connecting state, pair, forget, devices added.
ShellRoot {
    id: root
    property var errors: []
    property int step: 0
    property int ticks: 0
    property bool sawConnecting: false
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    function finish() {
        if (errors.length) console.log("FAILED: " + errors.join(", "))
        Qt.exit(errors.length ? 1 : 0)
    }
    function dev(name) { return Bluetooth.devices.values.find(d => d.deviceName === name) ?? null }
    readonly property BluetoothAdapter adapter: Bluetooth.defaultAdapter

    Process {
        id: mock
        property int n: 0
        command: ["gdbus", "call", "--system", "-d", "org.qsrs.Mock", "-o", "/org/qsrs/Mock", "-m", "org.qsrs.Mock.Step", String(n)]
    }
    Process {
        id: get
        property string out: ""
        property var args: []
        command: ["gdbus", "call", "--system", "-d", "org.bluez", "-o", args[0] ?? "/", "-m", "org.freedesktop.DBus.Properties.Get", args[1] ?? "", args[2] ?? ""]
        stdout: StdioCollector { onStreamFinished: get.out = text.trim() }
    }
    function busGet(path, iface, name) { get.out = ""; get.args = [path, iface, name]; get.running = true }

    Connections {
        target: root.dev("Mouse")
        function onStateChanged() { if (root.dev("Mouse").state === BluetoothDeviceState.Connecting) root.sawConnecting = true }
    }

    Timer {
        interval: 50
        repeat: true
        running: true
        onTriggered: {
            root.ticks++
            if (root.ticks > 160) { root.check("timeout at step " + root.step, false); root.finish(); return }
            const a = root.adapter, hp = root.dev("Headphones"), mouse = root.dev("Mouse")
            switch (root.step) {
            case 0:
                if (!a || !hp || !mouse) return
                root.check("enums", BluetoothAdapterState.Blocked === 4 && BluetoothDeviceState.Connecting === 3)
                root.check("toString", BluetoothDeviceState.toString(BluetoothDeviceState.Disconnecting) === "Disconnecting")
                root.check("adapter", a.name === "test-adapter" && a.enabled && a.state === BluetoothAdapterState.Enabled)
                root.check("adapter id", a.adapterId === "hci0" && a.dbusPath === "/org/bluez/hci0")
                root.check("adapter props", a.discoverableTimeout === 180 && a.pairable && !a.discovering)
                root.check("devices", Bluetooth.devices.values.length === 2 && a.devices.values.length === 2)
                root.check("adapters", Bluetooth.adapters.values.length === 1)
                root.check("headphones", hp.connected && hp.paired && hp.state === BluetoothDeviceState.Connected)
                root.check("headphones props", hp.address === "11:11:11:11:11:11" && hp.icon === "audio-headphones" && hp.name === "Headphones")
                root.check("battery", hp.batteryAvailable && Math.abs(hp.battery - 0.8) < 1e-6)
                root.check("mouse no battery", !mouse.batteryAvailable && !mouse.connected)
                root.check("device adapter", mouse.adapter === a)
                a.discovering = true
                mouse.trusted = true
                mouse.name = "My Mouse"
                mouse.connect()
                root.step = 1
                break
            case 1:
                if (!a.discovering || !mouse.connected) return
                root.check("connecting seen", root.sawConnecting)
                root.check("connected state", mouse.state === BluetoothDeviceState.Connected)
                root.busGet(mouse.dbusPath, "org.bluez.Device1", "Alias")
                root.step = 2
                break
            case 2:
                if (!get.out) return
                root.check("alias on bus " + get.out, get.out.includes("My Mouse"))
                root.busGet(mouse.dbusPath, "org.bluez.Device1", "Trusted")
                root.step = 3
                break
            case 3:
                if (!get.out) return
                root.check("trusted on bus " + get.out, get.out.includes("true"))
                mouse.pair()
                root.check("pairing", mouse.pairing)
                hp.disconnect()
                root.step = 4
                break
            case 4:
                if (!mouse.paired || mouse.pairing || hp.connected) return
                root.check("bonded", mouse.bonded)
                root.check("disconnected", hp.state === BluetoothDeviceState.Disconnected)
                hp.forget()
                mock.n = 1; mock.running = true
                root.step = 5
                break
            case 5:
                if (root.dev("Headphones") || !root.dev("Speaker")) return
                root.check("forgot", Bluetooth.devices.values.length === 2 && a.devices.values.length === 2)
                root.check("speaker adapter", root.dev("Speaker").adapter === a)
                mock.n = 2; mock.running = true
                root.step = 6
                break
            case 6:
                if (a.state !== BluetoothAdapterState.Disabling) return
                root.finish()
            }
        }
    }
}
