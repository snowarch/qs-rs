pragma Singleton
import QtQuick

// A singleton per generation: a reload must build a fresh one.
QtObject {
    property int touched: 0
}
