import QtQuick
import Quickshell
import Quickshell.Wayland

// Two BackgroundEffects on one window never ask niri for two effect objects: that is a protocol
// error that ends the connection (10-20 12:37, live, on a reload whose old generation had not
// let go of a window the new one took back). `holder` stands in for that old generation: it
// points at the same backing window. wrap.sh runs this in a nested niri and reads the log.
ShellRoot {
    FloatingWindow {
        id: win
        implicitWidth: 200
        implicitHeight: 100
        color: "transparent"
        BackgroundEffect.blurRegion: Region { item: box }
        Rectangle { id: box; width: 100; height: 50; color: "#80ff0000" }
    }
    QtObject {
        id: holder
        property QtObject _backingWindow: null
        BackgroundEffect.blurRegion: Region { width: 40; height: 40 }
    }
    Timer {
        interval: 400
        running: true
        onTriggered: {
            holder._backingWindow = win._backingWindow
            console.log("second holder attached")
        }
    }
    Timer {
        interval: 1200
        running: true
        onTriggered: {
            holder._backingWindow = null
            console.log("second holder let go")
        }
    }
}
