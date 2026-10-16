import QtQml
import Quickshell
import Quickshell.Networking

// org.freedesktop.NetworkManager.Device. nmManaged and autoconnect write through to
// NetworkManager and change when it reports the new value, like quickshell.
QtObject {
    id: root
    property int type: DeviceType.None
    property string name
    property string address
    readonly property QtObject networks: ObjectModel {}
    property bool connected: state === ConnectionState.Connected
    property int state: ConnectionState.Unknown
    property bool nmManaged: false
    property bool autoconnect: false
    property string dbusPath
    property int _lastFailReason: 0
    property bool _syncing: false

    readonly property string _iface: "org.freedesktop.NetworkManager.Device"
    onNmManagedChanged: _write("Managed", nmManaged, v => nmManaged = v)
    onAutoconnectChanged: _write("Autoconnect", autoconnect, v => autoconnect = v)
    function _write(prop, want, assign) {
        if (_syncing) return
        _syncing = true; assign(!want); _syncing = false
        NmCore.setBool(dbusPath, _iface, prop, want)
    }

    function disconnect() {
        if (state === ConnectionState.Disconnected) return console.error("Device", root, "is already disconnected")
        if (state === ConnectionState.Disconnecting) return console.error("Device", root, "is already disconnecting")
        NmCore.call(dbusPath, _iface, "Disconnect")
    }
}
