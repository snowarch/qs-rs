pragma Singleton
import QtQml

QtObject {
    enum Enum { Wpa3SuiteB192 = 0, Sae = 1, Wpa2Eap = 2, Wpa2Psk = 3, WpaEap = 4, WpaPsk = 5, StaticWep = 6, DynamicWep = 7, Leap = 8, Owe = 9, Open = 10, Unknown = 11 }
    function toString(value) { return ["WPA3 Suite B 192-bit", "WPA3", "WPA2 Enterprise", "WPA2", "WPA Enterprise", "WPA", "WEP", "Dynamic WEP", "LEAP", "OWE", "Open", "Unknown"][value] ?? "Unknown" }
}
