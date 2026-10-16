import QtQml

// Quickshell binds only tracked objects; qs-rs keeps every node bound, so tracking is a no-op.
QtObject {
    property list<QtObject> objects
}
