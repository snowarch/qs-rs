import QtQuick

QtObject {
    property QtObject store
    property QtObject peer: store ? store.child : null
    property int seen: { const v = Store1.v + Store3.v + Store6.v; if (!peer) console.log("TEARDOWN-EVAL"); return peer ? v - 2 : -1 }
}
