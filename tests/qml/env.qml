import QtQuick
import Quickshell

QtObject {
    Component.onCompleted: {
        const home = Quickshell.env("HOME")
        const missing = Quickshell.env("QSRS_SURELY_UNSET")
        Qt.exit(home.endsWith("/home") && missing === null ? 0 : 1)
    }
}
