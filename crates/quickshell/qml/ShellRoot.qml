import QtQml
import Quickshell

// Root of a shell file. `settings` mirrors the settings exposed by the Quickshell singleton.
Scope {
    readonly property QtObject settings: QtObject {
        property alias workingDirectory: _proxy.workingDirectory
        property alias watchFiles: _proxy.watchFiles
    }
    property QtObject _proxy: QtObject {
        id: _proxy
        property string workingDirectory: Quickshell.workingDirectory
        property bool watchFiles: Quickshell.watchFiles
        onWorkingDirectoryChanged: Quickshell.workingDirectory = workingDirectory
        onWatchFilesChanged: Quickshell.watchFiles = watchFiles
    }
}
