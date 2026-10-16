import QtQml
import Quickshell.Networking

NetworkDevice {
    type: DeviceType.Wired
    property Network network: null
    property int linkSpeed: 0
    property bool hasLink: false
    property var _net: null
}
