import QtQml

// Groups objects without creating anything visual. Children declared inside belong to it.
QtObject {
    property string reloadableId
    default property list<QtObject> children
}
