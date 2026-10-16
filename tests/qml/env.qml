import QtQuick
import Quickshell

QtObject {
    Component.onCompleted: {
        const home = Quickshell.env("HOME")
        console.log("HOME=" + home)
        Qt.exit(home.endsWith("/home") ? 0 : 1)
    }
}
