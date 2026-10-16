pragma Singleton
import QtQml
import Quickshell.Bluetooth

// BlueZ objects from BluezCore's object manager stream, one QML object per D-Bus path.
// The default adapter is the first one, like quickshell.
QtObject {
    id: root
    readonly property BluetoothAdapter defaultAdapter: adapters.values[0] ?? null
    readonly property QtObject adapters: QtObject { property list<BluetoothAdapter> values }
    readonly property QtObject devices: QtObject { property list<BluetoothDevice> values }

    property var _adapters: ({})
    property var _devices: ({})
    property Component _adapterComponent: Component { BluetoothAdapter {} }
    property Component _deviceComponent: Component { BluetoothDevice {} }

    function _sorted(map) { return Object.keys(map).sort().map(k => map[k]) }
    function _relink() {
        adapters.values = _sorted(_adapters)
        devices.values = _sorted(_devices)
        for (const a of adapters.values) a.devices.values = devices.values.filter(d => d._adapterPath === a.dbusPath)
        for (const d of devices.values) if (d.adapter !== (_adapters[d._adapterPath] ?? null)) d.adapter = _adapters[d._adapterPath] ?? null
    }

    property Connections _core: Connections {
        target: BluezCore
        function onInterfacesAdded(path, json) {
            const ifaces = JSON.parse(json)
            let changed = false
            if (ifaces["org.bluez.Adapter1"]) {
                let a = root._adapters[path]
                if (!a) { a = root._adapterComponent.createObject(root, { dbusPath: path }); root._adapters[path] = a; changed = true }
                a._update(ifaces["org.bluez.Adapter1"])
            }
            let d = root._devices[path]
            if (ifaces["org.bluez.Device1"]) {
                if (!d) { d = root._deviceComponent.createObject(root, { dbusPath: path }); root._devices[path] = d; changed = true }
                d._update(ifaces["org.bluez.Device1"])
            }
            if (d && ifaces["org.bluez.Battery1"]) {
                d.batteryAvailable = true
                d._updateBattery(ifaces["org.bluez.Battery1"])
            }
            if (changed) root._relink()
        }
        function onInterfacesRemoved(path, ifaces) {
            const d = root._devices[path], a = root._adapters[path]
            if (d && ifaces.includes("org.bluez.Battery1")) d.batteryAvailable = false
            if (d && ifaces.includes("org.bluez.Device1")) { delete root._devices[path]; root._relink(); d.destroy() }
            if (a && ifaces.includes("org.bluez.Adapter1")) { delete root._adapters[path]; root._relink(); a.destroy() }
        }
        function onPropertiesChanged(path, iface, json) {
            const p = JSON.parse(json)
            if (iface === "org.bluez.Adapter1") root._adapters[path]?._update(p)
            else if (iface === "org.bluez.Device1") {
                const d = root._devices[path]
                if (!d) return
                const oldAdapter = d._adapterPath
                d._update(p)
                if (d._adapterPath !== oldAdapter) root._relink()
            } else if (iface === "org.bluez.Battery1") root._devices[path]?._updateBattery(p)
        }
        function onCallFinished(path, method, error) { root._devices[path]?._finished(method, error) }
    }
}
