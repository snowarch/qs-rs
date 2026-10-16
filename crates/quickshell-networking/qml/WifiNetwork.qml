import QtQml
import Quickshell.Networking

Network {
    id: root
    property real signalStrength: 0
    property int security: WifiSecurityType.Unknown

    function connectWithPsk(psk) {
        if (connected) return console.error(root, "is already connected.")
        if (security !== WifiSecurityType.WpaPsk && security !== WifiSecurityType.Wpa2Psk && security !== WifiSecurityType.Sae)
            return console.error(root, "has the wrong security type for a PSK.")
        if (_referenceSettings) NmCore.updatePskAndActivate(_referenceSettings, device.dbusPath, psk)
        else if (_referenceAp) NmCore.addAndActivate(device.dbusPath, _referenceAp, psk)
        else console.info("Failed to connectWithPsk: The network disappeared.")
    }
}
