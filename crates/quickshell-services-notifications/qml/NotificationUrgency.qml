pragma Singleton
import QtQml

QtObject {
    enum Enum { Low = 0, Normal = 1, Critical = 2 }
    function toString(value) { return ["Low", "Normal", "Critical"][value] ?? "Invalid notification urgency" }
}
