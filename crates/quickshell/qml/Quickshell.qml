pragma Singleton
import QtQuick
import Quickshell

// The `Quickshell` singleton. Paths, environment and processes come from the Rust core
// (QuickshellCore); screens are tracked here so each monitor keeps one stable object.
QtObject {
    id: root
    readonly property string shellDir: QuickshellCore.shellDir
    readonly property string configDir: QuickshellCore.shellDir
    readonly property string shellRoot: QuickshellCore.shellDir
    readonly property string dataDir: QuickshellCore.dataDir
    readonly property string stateDir: QuickshellCore.stateDir
    readonly property string cacheDir: QuickshellCore.cacheDir
    readonly property string shellId: QuickshellCore.shellId
    readonly property int processId: QuickshellCore.processId
    property string workingDirectory: QuickshellCore.workingDirectory
    onWorkingDirectoryChanged: if (QuickshellCore.workingDirectory !== workingDirectory) QuickshellCore.workingDirectory = workingDirectory
    property bool watchFiles: QuickshellCore.watchFiles
    onWatchFilesChanged: if (QuickshellCore.watchFiles !== watchFiles) QuickshellCore.watchFiles = watchFiles
    property string clipboardText: QsClipboard.text
    onClipboardTextChanged: if (clipboardText !== QsClipboard.text) QsClipboard.text = clipboardText
    // A JS array, not list<QtObject>: see Toplevel.screens (the old generation dies on every reload).
    readonly property var screens: _screens

    property var _screens: []
    property var _byName: ({})
    property Component _screenComponent: Component { ShellScreen {} }

    function _updateScreens() {
        const infos = Qt.application.screens
        const next = []
        const seen = {}
        for (let i = 0; i < infos.length; i++) {
            const info = infos[i]
            let s = _byName[info.name]
            if (!s) s = _screenComponent.createObject(root, { _info: info })
            else s._info = info
            seen[info.name] = s
            next.push(s)
        }
        for (const name in _byName) if (!seen[name]) _byName[name].destroy()
        _byName = seen
        _screens = next
    }

    property Connections _screensWatcher: Connections {
        target: Qt.application
        function onScreensChanged() { root._updateScreens() }
    }

    Component.onCompleted: _updateScreens()

    signal reloadCompleted()
    signal reloadFailed(string errorString)

    function env(name) { return QuickshellCore.env(name) }
    function execDetached(context) { QuickshellCore.execDetached(context) }
    function shellPath(path) { return QuickshellCore.shellPath(path) }
    function configPath(path) { return QuickshellCore.shellPath(path) }
    function dataPath(path) { return QuickshellCore.dataPath(path) }
    function statePath(path) { return QuickshellCore.statePath(path) }
    function cachePath(path) { return QuickshellCore.cachePath(path) }
    function iconPath(icon, extra) {
        return extra === undefined ? QuickshellCore.iconPath(icon) : QuickshellCore.iconPath(icon, extra)
    }
    function hasThemeIcon(icon) { return QuickshellCore.hasThemeIcon(icon) }
    function hasVersion(major, minor, features) {
        return features === undefined ? QuickshellCore.hasVersion(major, minor) : QuickshellCore.hasVersion(major, minor, features)
    }
    function hasQtVersion(major, minor) { return QuickshellCore.hasQtVersion(major, minor) }
    function reload(hard) { QuickshellCore.reload(!!hard) }
    function inhibitReloadPopup() {}
}
