pragma Singleton
import QtQml

QtObject {
    enum Enum { None = 0, NetworkManager = 1 }
    function toString(value) { return ["None", "NetworkManager"][value] ?? "Unknown" }
}
