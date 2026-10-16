import QtQml
import Quickshell.Networking

// While scannerEnabled, asks for a scan at most every 10 s, counting NetworkManager's own
// scans (LastScan), like quickshell.
NetworkDevice {
    id: root
    type: DeviceType.Wifi
    property bool scannerEnabled: false
    property int mode: WifiDeviceMode.Unknown
    property real _lastScan: -1
    property real _lastRequest: -1
    property var _nets: ({})
    property var _order: []

    readonly property int _scanInterval: 10001
    property Timer _scanTimer: Timer { onTriggered: root._scanTick() }
    onScannerEnabledChanged: {
        if (scannerEnabled) _scanTick(); else _scanTimer.stop()
        Networking._schedule()
    }
    function _scanTick() {
        const now = NmCore.bootTimeMs()
        const wait = t => { _scanTimer.interval = Math.max(1, _scanInterval - (now - t)); _scanTimer.restart() }
        if (_lastScan >= 0 && now - _lastScan < _scanInterval) wait(_lastScan)
        else if (_lastRequest >= 0 && now - _lastRequest < _scanInterval) wait(_lastRequest)
        else {
            NmCore.requestScan(dbusPath)
            _lastRequest = now
            wait(now)
        }
    }
}
