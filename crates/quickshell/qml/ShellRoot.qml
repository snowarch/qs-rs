import QtQml
import Quickshell

// Root of a shell file. `settings` exposes workingDirectory and watchFiles, shared with the
// Quickshell singleton.
Scope {
    readonly property QtObject settings: Quickshell
}
