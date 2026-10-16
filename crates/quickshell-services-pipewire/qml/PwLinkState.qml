pragma Singleton
import QtQml

// pw_link_state values.
QtObject {
    enum Enum {
        Error = -2,
        Unlinked = -1,
        Init = 0,
        Negotiating = 1,
        Allocating = 2,
        Paused = 3,
        Active = 4
    }

    function toString(state) {
        return ({ "-2": "Error", "-1": "Unlinked", 0: "Init", 1: "Negotiating", 2: "Allocating", 3: "Paused", 4: "Active" })[state] ?? "Invalid"
    }
}
