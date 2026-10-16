pragma Singleton
import QtQml

QtObject {
    enum Enum { None = 0, Wifi = 1, Wired = 2 }
    function toString(value) { return ["None", "Wifi", "Wired"][value] ?? "Unknown" }
}
