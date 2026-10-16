pragma Singleton
import QtQml

QtObject {
    enum Enum { Unknown = 0, None = 1, Portal = 2, Limited = 3, Full = 4 }
    function toString(value) { return ["Unknown", "Not connected to a network", "Connection intercepted by a captive portal", "Partial internet connectivity", "Full internet connectivity"][value] ?? "Unknown" }
}
