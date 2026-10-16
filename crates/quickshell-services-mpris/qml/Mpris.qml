pragma Singleton
import QtQml
import Quickshell.Services.Mpris

// Players by bus name; a player joins `players` once both of its interfaces were read.
QtObject {
    id: root
    readonly property QtObject players: QtObject { property list<MprisPlayer> values }

    property var _players: ({})
    property var _pending: ({})
    property Component _playerComponent: Component { MprisPlayer {} }

    function _sync() { players.values = Object.values(_players).filter(p => p.ready) }

    property Connections _core: Connections {
        target: MprisCore
        function onPropertiesChanged(name, iface, json) {
            let p = root._players[name]
            if (!p) { p = root._playerComponent.createObject(root, { dbusName: name }); root._players[name] = p; root._pending[name] = {} }
            p._update(iface, JSON.parse(json))
            if (!p.ready) {
                root._pending[name][iface] = true
                if (root._pending[name]["org.mpris.MediaPlayer2"] && root._pending[name]["org.mpris.MediaPlayer2.Player"]) {
                    delete root._pending[name]
                    p.ready = true
                    root._sync()
                }
            }
        }
        function onSeeked(name, us) { root._players[name]?._setPositionUs(us) }
        function onPlayerRemoved(name) {
            const p = root._players[name]
            if (!p) return
            delete root._players[name]
            delete root._pending[name]
            root._sync()
            p.destroy()
        }
    }
}
