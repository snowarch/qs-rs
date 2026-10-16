import QtQuick
import qs.services

QtObject {
    Component.onCompleted: Qt.exit(Counter.value === 41 && Counter.bump() === 42 ? 0 : 1)
}
