pragma Singleton
import QtQml

QtObject {
    enum Enum { Disabled = 0, Enabled = 1, Enabling = 2, Disabling = 3, Blocked = 4 }
    function toString(state) { return ["Disabled", "Enabled", "Enabling", "Disabling", "Blocked"][state] ?? "Unknown" }
    function _fromWire(w) { return ({ "off": 0, "on": 1, "off-enabling": 2, "on-disabling": 3, "off-blocked": 4 })[w] ?? 0 }
}
