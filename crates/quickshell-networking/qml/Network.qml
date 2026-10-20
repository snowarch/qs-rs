import QtQml
import Quickshell.Networking

// A network a device can connect to: a Wi-Fi SSID or a wired link. connect() activates the
// reference profile (the active one, else the last used), or asks NetworkManager to make one.
QtObject {
    id: root
    property string name
    property NetworkDevice device: null
    // A JS array, not list<NMSettings>: see Toplevel.screens (a vanished network is destroyed).
    property var nmSettings: []
    property bool connected: state === ConnectionState.Connected
    property bool known: false
    property int state: ConnectionState.Disconnected
    readonly property bool stateChanging: state === ConnectionState.Connecting || state === ConnectionState.Disconnecting
    signal connectionFailed(int reason)

    property string _activePath
    property string _referenceSettings
    property string _referenceAp

    function connect() {
        if (connected) return console.error(root, "is already connected.")
        if (_referenceSettings) NmCore.activate(_referenceSettings, device.dbusPath, "/")
        else NmCore.addAndActivate(device.dbusPath, _referenceAp || "/", "")
    }
    function connectWithSettings(settings) {
        if (connected) return console.error(root, "is already connected.")
        if (Array.prototype.indexOf.call(nmSettings, settings) === -1) return
        NmCore.activate(settings.dbusPath, device.dbusPath, "/")
    }
    function disconnect() {
        if (!connected) return console.error(root, "is not currently connected")
        NmCore.call(device.dbusPath, "org.freedesktop.NetworkManager.Device", "Disconnect")
    }
    function forget() { for (const s of nmSettings) s.forget() }
}
