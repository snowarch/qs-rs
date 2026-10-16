pragma Singleton
import QtQml

QtObject {
    enum Enum { Unknown = 0, NoSecrets = 1, WifiClientDisconnected = 2, WifiClientFailed = 3, WifiAuthTimeout = 4, WifiNetworkLost = 5 }
    function toString(value) { return ["Unknown", "Secrets were required but not provided", "Wi-Fi supplicant disconnected", "Wi-Fi supplicant failed", "Wi-Fi connection took too long to authenticate", "Wi-Fi network could not be found"][value] ?? "Unknown" }
}
