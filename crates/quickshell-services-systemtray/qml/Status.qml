pragma Singleton
import QtQml

QtObject {
    enum Enum { Passive = 0, Active = 1, NeedsAttention = 2 }
    function toString(value) { return ["Passive", "Active", "NeedsAttention"][value] ?? "Invalid status" }
    function _fromWire(w) { return ({ "Passive": 0, "Active": 1, "NeedsAttention": 2 })[w] ?? 1 }
}
