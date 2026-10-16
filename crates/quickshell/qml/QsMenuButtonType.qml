pragma Singleton
import QtQml

QtObject {
    enum Enum { None = 0, CheckBox = 1, RadioButton = 2 }
    function toString(value) {
        return ["None", "CheckBox", "RadioButton"][value] ?? "Invalid button type"
    }
}
