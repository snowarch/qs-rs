pragma Singleton
import QtQml
import Quickshell.Services.UPower

// Devices from UPowerCore, kept by object path. `devices` holds devices once their first
// property read finished, like quickshell; the display device is separate and always exists.
QtObject {
    id: root
    readonly property UPowerDevice displayDevice: UPowerDevice {}
    readonly property QtObject devices: QtObject { property list<UPowerDevice> values }
    property bool onBattery: false

    property string _displayPath
    property var _devices: ({})
    property Component _deviceComponent: Component { UPowerDevice {} }

    property Connections _core: Connections {
        target: UPowerCore
        function onDisplayDevice(path) {
            root._displayPath = path
            root.displayDevice.ready = true
        }
        function onPropertiesChanged(path, json) {
            const p = JSON.parse(json)
            if (path === "/") {
                if (p.OnBattery !== undefined) root.onBattery = p.OnBattery
                return
            }
            if (path === root._displayPath || path.endsWith("/DisplayDevice")) {
                root.displayDevice._update(p)
                return
            }
            let d = root._devices[path]
            if (!d) {
                d = root._deviceComponent.createObject(root, { _path: path })
                root._devices[path] = d
                d._update(p)
                d.ready = true
                root.devices.values = Object.values(root._devices)
                return
            }
            d._update(p)
        }
        function onDeviceRemoved(path) {
            const d = root._devices[path]
            if (!d) return
            delete root._devices[path]
            root.devices.values = Object.values(root._devices)
            d.destroy()
        }
    }
}
