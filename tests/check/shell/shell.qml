import QtQuick
import Quickshell
import "areas"

// The broken area is never instantiated: only `check` can see its error.
ShellRoot {
	Good {}
	LazyLoader { active: false; component: Broken {} }
}
