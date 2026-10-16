pragma Singleton
import QtQml

QtObject {
    enum Enum { Stopped = 0, Playing = 1, Paused = 2 }
    function toString(state) { return ["Stopped", "Playing", "Paused"][state] ?? "Unknown Status" }
    function _fromWire(w) { return ({ "Stopped": 0, "Playing": 1, "Paused": 2 })[w] ?? 0 }
}
