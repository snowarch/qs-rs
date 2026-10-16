pragma Singleton
import QtQml

QtObject {
    enum Enum { Expired = 1, Dismissed = 2, CloseRequested = 3 }
    function toString(value) { return [undefined, "Expired", "Dismissed", "CloseRequested"][value] ?? "Invalid notification close reason" }
}
