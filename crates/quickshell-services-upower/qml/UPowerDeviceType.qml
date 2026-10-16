pragma Singleton
import QtQml

QtObject {
    enum Enum {
        Unknown = 0, LinePower = 1, Battery = 2, Ups = 3, Monitor = 4, Mouse = 5, Keyboard = 6, Pda = 7,
        Phone = 8, MediaPlayer = 9, Tablet = 10, Computer = 11, GamingInput = 12, Pen = 13, Touchpad = 14,
        Modem = 15, Network = 16, Headset = 17, Speakers = 18, Headphones = 19, Video = 20, OtherAudio = 21,
        RemoteControl = 22, Printer = 23, Scanner = 24, Camera = 25, Wearable = 26, Toy = 27, BluetoothGeneric = 28
    }
    function toString(type) {
        return ["Unknown", "Line Power", "Battery", "Ups", "Monitor", "Mouse", "Keyboard", "Pda", "Phone",
                "Media Player", "Tablet", "Computer", "Gaming Input", "Pen", "Touchpad", "Modem", "Network",
                "Headset", "Speakers", "Headphones", "Video", "Other Audio", "Remote Control", "Printer",
                "Scanner", "Camera", "Wearable", "Toy", "Bluetooth Generic"][type] ?? "Invalid Type"
    }
}
