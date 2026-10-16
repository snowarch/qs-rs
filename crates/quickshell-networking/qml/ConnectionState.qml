pragma Singleton
import QtQml

QtObject {
    enum Enum { Unknown = 0, Connecting = 1, Connected = 2, Disconnecting = 3, Disconnected = 4 }
    function toString(value) { return ["Unknown", "Connecting", "Connected", "Disconnecting", "Disconnected"][value] ?? "Unknown" }
}
