import QtQml
import Quickshell
import Quickshell.Services.Pam

// A relative configDirectory resolves against the file that declares the context, like
// upstream's resolvedUrl in setConfigDirectory.
PamContextBase {
    id: root
    _qsConfigPath: {
        const dir = configDirectory
        if (dir.startsWith("/")) return dir
        const url = QsWindowSupport.resolvedUrl(root, dir).toString()
        return url.startsWith("file://") ? decodeURIComponent(url.slice(7)) : url
    }
    Component.onCompleted: _qsComplete()
}
