//@ pragma DefaultEnv QS_DROP_EXPENSIVE_FONTS=1
import QtQuick
import Quickshell
import Quickshell.Io

// inir's DefaultEnv QS_DROP_EXPENSIVE_FONTS=1 (shell.qml:10): as in upstream, fontconfig gets a
// filter in the instance run dir that includes the base config and rejects woff/woff2 fonts.
ShellRoot {
    id: root
    readonly property string path: Quickshell.env("FONTCONFIG_FILE") ?? ""
    FileView {
        id: conf
        path: root.path
        blockLoading: true
    }
    Component.onCompleted: {
        const text = root.path.endsWith("/fonts-override.conf") ? conf.text() : ""
        const ok = text.includes("<include ignore_missing=\"no\">/etc/fonts/fonts.conf</include>")
            && text.includes("<string>woff</string>") && text.includes("<string>woff2</string>")
        if (!ok) console.log("FAILED: FONTCONFIG_FILE=" + root.path + " text=" + text)
        Qt.exit(ok ? 0 : 1)
    }
}
