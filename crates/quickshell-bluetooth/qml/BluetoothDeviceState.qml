pragma Singleton
import QtQml

QtObject {
    enum Enum { Disconnected = 0, Connected = 1, Disconnecting = 2, Connecting = 3 }
    function toString(state) { return ["Disconnected", "Connected", "Disconnecting", "Connecting"][state] ?? "Unknown" }
}
