pragma Singleton
import QtQuick

// Keeps a parentless JS object alive: it is no child of any singleton, so the engine teardown
// does not mark it deleted, and its bindings still run while the singletons go one by one.
QtObject {
    readonly property var probes: [probe.createObject(null, { store: Store6 })]
    readonly property int seen: probes[0].seen
    property Component probe: Component { Probe {} }
}
