pragma Singleton
import QtQml

QtObject {
    enum Enum { None = 0, LapDetected = 1, HighTemperature = 2 }
    function toString(reason) { return ["None", "Lap Detected", "High Temperature"][reason] ?? "Invalid" }
}
