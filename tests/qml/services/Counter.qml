pragma Singleton
import QtQuick

QtObject {
    property int value: 41
    function bump() { return ++value }
}
