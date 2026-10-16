pragma Singleton
import QtQml

QtObject {
    enum Enum { None = 0, Track = 1, Playlist = 2 }
    function toString(state) { return ["None", "Track", "Playlist"][state] ?? "Unknown Status" }
    readonly property var _wire: ["None", "Track", "Playlist"]
}
