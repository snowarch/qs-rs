import QtQuick
import Quickshell

// Driven by tests/quitsignal/wrap.sh: SIGTERM quits the event loop (exit 0) instead of killing.
ShellRoot { Component.onCompleted: console.log("READY") }
