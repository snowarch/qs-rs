pragma Singleton
import QtQml

QtObject {
    enum Enum { Hardware = 0, SystemServices = 1, ApplicationStatus = 2, Communications = 3 }
    function toString(value) { return ["Hardware", "SystemServices", "ApplicationStatus", "Communications"][value] ?? "Invalid category" }
    function _fromWire(w) { return ({ "Hardware": 0, "SystemServices": 1, "ApplicationStatus": 2, "Communications": 3 })[w] ?? 2 }
}
