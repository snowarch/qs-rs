pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland

Variants {
    model: Quickshell.screens

    delegate: Scope {
        id: screenScope
        required property var modelData
        readonly property string outName: screenScope.modelData?.name ?? ""
        function depth(edge: string): real { return edge === "top" ? 3 : 1 }

        component Stub: PanelWindow {
            id: stub
            required property string edge
            screen: screenScope.modelData
            visible: true
            color: "transparent"
            WlrLayershell.namespace: "test-" + stub.edge
            exclusiveZone: Math.max(0, screenScope.depth(stub.edge))
            property string name: screenScope.outName
            implicitWidth: 1
            implicitHeight: 1
        }

        Stub { edge: "top" }
        Stub { edge: "left" }
    }
}
