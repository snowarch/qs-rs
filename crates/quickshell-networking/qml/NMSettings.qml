import QtQml
import Quickshell.Networking

// A saved NetworkManager profile (org.freedesktop.NetworkManager.Settings.Connection).
QtObject {
    id: root
    property string dbusPath
    readonly property string id: _map.connection?.id ?? ""
    readonly property string uuid: _map.connection?.uuid ?? ""
    property var _map: ({})
    property bool _loaded: false

    readonly property string _iface: "org.freedesktop.NetworkManager.Settings.Connection"
    function read() { return _map }
    function write(settings) { console.warn("NMSettings.write is not supported by qs-rs yet") }
    function clearSecrets() { NmCore.call(dbusPath, _iface, "ClearSecrets") }
    function forget() { NmCore.call(dbusPath, _iface, "Delete") }
}
