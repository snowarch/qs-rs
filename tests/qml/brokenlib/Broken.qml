import QtQuick

// Fails to load on purpose: a Loader reports "Missing is not a type" with this file's line.
Item {
    Missing {}
}
