pragma Singleton
import QtQml

QtObject {
    enum Enum { PowerSaver = 0, Balanced = 1, Performance = 2 }
    function toString(profile) { return ["Power Saver", "Balanced", "Performance"][profile] ?? "Invalid" }
    readonly property var _wire: ["power-saver", "balanced", "performance"]
}
