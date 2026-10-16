import QtQml
import Quickshell.Bluetooth

// org.bluez.Device1 (+ Battery1). connect()/disconnect() show Connecting/Disconnecting until
// BlueZ reports the result, like quickshell.
QtObject {
    id: root
    property string address
    property string name
    property string deviceName
    property string icon
    property int state: BluetoothDeviceState.Disconnected
    property bool connected: false
    property bool paired: false
    property bool bonded: false
    property bool pairing: false
    property bool trusted: false
    property bool blocked: false
    property bool wakeAllowed: false
    property bool batteryAvailable: false
    property real battery: 0
    property BluetoothAdapter adapter: null
    property string dbusPath
    property string _adapterPath
    property bool _syncing: false

    readonly property string _iface: "org.bluez.Device1"
    onNameChanged: if (!_syncing) BluezCore.setString(dbusPath, _iface, "Alias", name)
    onTrustedChanged: if (!_syncing) BluezCore.setBool(dbusPath, _iface, "Trusted", trusted)
    onBlockedChanged: if (!_syncing) BluezCore.setBool(dbusPath, _iface, "Blocked", blocked)
    onWakeAllowedChanged: if (!_syncing) BluezCore.setBool(dbusPath, _iface, "WakeAllowed", wakeAllowed)
    onConnectedChanged: {
        if (_syncing) return
        const want = connected
        _set(() => connected = !want)
        if (want) connect(); else disconnect()
    }

    function _set(f) { _syncing = true; f(); _syncing = false }

    function connect() {
        if (connected) return console.warn("Device", name, "is already connected")
        if (state === BluetoothDeviceState.Connecting) return console.warn("Device", name, "is already connecting")
        state = BluetoothDeviceState.Connecting
        BluezCore.call(dbusPath, _iface, "Connect", "")
    }
    function disconnect() {
        if (!connected) return console.warn("Device", name, "is already disconnected")
        if (state === BluetoothDeviceState.Disconnecting) return console.warn("Device", name, "is already disconnecting")
        state = BluetoothDeviceState.Disconnecting
        BluezCore.call(dbusPath, _iface, "Disconnect", "")
    }
    function pair() {
        if (paired) return console.warn("Device", name, "is already paired")
        if (pairing) return console.warn("Device", name, "is already pairing")
        pairing = true
        BluezCore.call(dbusPath, _iface, "Pair", "")
    }
    function cancelPair() {
        if (!pairing) return console.warn("Device", name, "is not currently pairing")
        BluezCore.call(dbusPath, _iface, "CancelPairing", "")
    }
    function forget() { BluezCore.call(_adapterPath, "org.bluez.Adapter1", "RemoveDevice", dbusPath) }

    function _finished(method, error) {
        if (method === "Pair") pairing = false
        else if (error && (method === "Connect" || method === "Disconnect"))
            state = connected ? BluetoothDeviceState.Connected : BluetoothDeviceState.Disconnected
    }

    function _update(p) {
        _syncing = true
        if (p.Address !== undefined) address = p.Address
        if (p.Name !== undefined) deviceName = p.Name
        if (p.Alias !== undefined) name = p.Alias
        if (p.Icon !== undefined) icon = p.Icon
        if (p.Paired !== undefined) paired = p.Paired
        if (p.Bonded !== undefined) bonded = p.Bonded
        if (p.Trusted !== undefined) trusted = p.Trusted
        if (p.Blocked !== undefined) blocked = p.Blocked
        if (p.WakeAllowed !== undefined) wakeAllowed = p.WakeAllowed
        if (p.Adapter !== undefined) _adapterPath = p.Adapter
        if (p.Connected !== undefined) {
            connected = p.Connected
            state = connected ? BluetoothDeviceState.Connected : BluetoothDeviceState.Disconnected
        }
        _syncing = false
    }
    function _updateBattery(p) { if (p.Percentage !== undefined) battery = p.Percentage * 0.01 }
}
