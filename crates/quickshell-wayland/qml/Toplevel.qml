import QtQml
import Quickshell
import Quickshell.Wayland

// A window from zwlr_foreign_toplevel_management_v1, created by ToplevelManager.
ToplevelBase {
    id: root
    readonly property string appId: _appId
    readonly property string title: _title
    readonly property bool activated: _activated
    readonly property Toplevel parent: _parent
    // JS arrays, not list<QtObject>: a list property dies with its owner, and a JS value still holding the
    // list (a model built from toplevels) segfaults Qt when read after the window closes.
    readonly property var screens: _screens

    signal closed()

    property string _appId
    property string _title
    property bool _activated
    property Toplevel _parent: null
    property var _screens: []

    function fullscreenOn(screen) { _qsFullscreenOn(screen?.name ?? "") }
    // Not implemented: inir never sets a minimize rectangle.
    function setRectangle(window, rect) {}
    function unsetRectangle() {}
    function toString() { return "Toplevel(" + _appId + ")" }
}
