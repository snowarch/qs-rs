//@ pragma EnableQtWebEngineQuick
import QtQuick
import QtWebEngine
import Quickshell

// inir's webapps: a WebEngineView loads a page and runs script. The pragma calls
// QtWebEngineQuick::initialize() before the app like inir's patched Quickshell; since Qt 6.9 that
// only picks the graphics API (6.8 and older built a shared GL context there), so qs-rs skips it.
ShellRoot {
    // inir offers webapps only where the runtime advertises them (stock Quickshell: false).
    Component.onCompleted: {
        const ok = Quickshell.hasVersion(0, 3, ["webengine"]) && !Quickshell.hasVersion(0, 3, ["webengine", "nope"])
            && Quickshell.hasVersion(0, 2, ["nope"]) && !Quickshell.hasVersion(0, 4, ["webengine"])
        if (!ok) { console.log("FAILED hasVersion features"); Qt.exit(1) }
    }
    FloatingWindow {
        implicitWidth: 320; implicitHeight: 200
        WebEngineView {
            id: view
            anchors.fill: parent
            profile: WebEngineProfile { offTheRecord: true }
            url: "data:text/html,<title>qs-rs</title><body style='background:%23336'>hi</body>"
            onLoadingChanged: req => {
                if (req.status === WebEngineView.LoadFailedStatus) { console.log("FAILED load", req.errorString); Qt.exit(1) }
                if (req.status !== WebEngineView.LoadSucceededStatus) return
                view.runJavaScript("document.title + ':' + (1 + 1)", r => {
                    if (r !== "qs-rs:2") console.log("FAILED script", r)
                    Qt.exit(r === "qs-rs:2" ? 0 : 1)
                })
            }
        }
    }
}
