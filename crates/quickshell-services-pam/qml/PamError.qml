pragma Singleton
import QtQml

QtObject {
    enum Enum { StartFailed = 1, TryAuthFailed = 2, InternalError = 3 }
    function toString(value) {
        return [undefined, "Failed to start the PAM session", "Failed to try authenticating",
                "Internal error occurred"][value] ?? "Invalid error"
    }
}
