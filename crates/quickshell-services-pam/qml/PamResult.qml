pragma Singleton
import QtQml

QtObject {
    enum Enum { Success = 0, Failed = 1, Error = 2, MaxTries = 3 }
    function toString(value) {
        return ["Success", "Failed", "Error occurred while authenticating",
                "The authentication method has no more attempts available"][value] ?? "Invalid result"
    }
}
