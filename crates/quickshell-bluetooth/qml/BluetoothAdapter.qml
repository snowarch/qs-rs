import QtQml
import Quickshell.Bluetooth

// org.bluez.Adapter1. Writable properties are sent to BlueZ; the confirmed value comes back
// through PropertiesChanged.
QtObject {
    id: root
    property string name
    property bool enabled: false
    property int state: BluetoothAdapterState.Disabled
    property bool discoverable: false
    property int discoverableTimeout: 0
    property bool discovering: false
    property bool pairable: false
    property int pairableTimeout: 0
    readonly property QtObject devices: QtObject { property list<BluetoothDevice> values }
    readonly property string adapterId: dbusPath.slice(dbusPath.lastIndexOf("/") + 1)
    property string dbusPath
    property bool _syncing: false

    readonly property string _iface: "org.bluez.Adapter1"
    onEnabledChanged: if (!_syncing) BluezCore.setBool(dbusPath, _iface, "Powered", enabled)
    onDiscoverableChanged: if (!_syncing) BluezCore.setBool(dbusPath, _iface, "Discoverable", discoverable)
    onDiscoverableTimeoutChanged: if (!_syncing) BluezCore.setUInt(dbusPath, _iface, "DiscoverableTimeout", discoverableTimeout)
    onPairableChanged: if (!_syncing) BluezCore.setBool(dbusPath, _iface, "Pairable", pairable)
    onPairableTimeoutChanged: if (!_syncing) BluezCore.setUInt(dbusPath, _iface, "PairableTimeout", pairableTimeout)
    onDiscoveringChanged: if (!_syncing) BluezCore.call(dbusPath, _iface, discovering ? "StartDiscovery" : "StopDiscovery", "")

    function _update(p) {
        _syncing = true
        if (p.Alias !== undefined) name = p.Alias
        if (p.Powered !== undefined) enabled = p.Powered
        if (p.PowerState !== undefined) state = BluetoothAdapterState._fromWire(p.PowerState)
        if (p.Discoverable !== undefined) discoverable = p.Discoverable
        if (p.DiscoverableTimeout !== undefined) discoverableTimeout = p.DiscoverableTimeout
        if (p.Discovering !== undefined) discovering = p.Discovering
        if (p.Pairable !== undefined) pairable = p.Pairable
        if (p.PairableTimeout !== undefined) pairableTimeout = p.PairableTimeout
        _syncing = false
    }
}
