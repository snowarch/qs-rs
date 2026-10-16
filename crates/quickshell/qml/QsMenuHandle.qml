import QtQml

// Upstream QsMenuHandle: something a QsMenuOpener can show. `menu` is the entry whose children
// are shown, null until it is loaded; the opener holds a reference while it shows the handle.
QtObject {
    property QtObject menu: null
    function refHandle() {}
    function unrefHandle() {}
}
