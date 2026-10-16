pragma Singleton
import QtQml

QtObject {
    enum Enum { AdHoc = 0, Station = 1, AccessPoint = 2, Mesh = 3, Unknown = 4 }
    function toString(value) { return ["Ad-Hoc", "Station", "Access Point", "Mesh", "Unknown"][value] ?? "Unknown" }
}
