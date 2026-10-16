pragma Singleton
import QtQml

QtObject {
    enum Enum { Unknown = 0, Charging = 1, Discharging = 2, Empty = 3, FullyCharged = 4, PendingCharge = 5, PendingDischarge = 6 }
    function toString(state) {
        return ["Unknown", "Charging", "Discharging", "Empty", "Fully Charged", "Pending Charge", "Pending Discharge"][state] ?? "Invalid Status"
    }
}
