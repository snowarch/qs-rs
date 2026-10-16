pragma Singleton
import QtQml
import Quickshell
import Quickshell.Networking

// NetworkManager's objects from NmCore's object manager stream, rebuilt into devices and
// networks once per event batch (Qt.callLater), like quickshell's NM backend: Wi-Fi networks
// group access points and saved profiles by SSID and show while scanning, connected or known.
QtObject {
    id: root
    readonly property QtObject devices: ObjectModel {}
    readonly property int backend: _backend
    property bool wifiEnabled: false
    readonly property bool wifiHardwareEnabled: _nm.WirelessHardwareEnabled ?? false
    readonly property bool canCheckConnectivity: _nm.ConnectivityCheckAvailable ?? false
    property bool connectivityCheckEnabled: false
    readonly property int connectivity: _nm.Connectivity ?? NetworkConnectivity.Unknown

    function checkConnectivity() {
        if (!connectivityCheckEnabled || !canCheckConnectivity) return
        NmCore.call(_nmPath, _nmIface, "CheckConnectivity")
    }

    property int _backend: NetworkBackendType.None
    property var _nm: ({})
    property bool _syncing: false
    onWifiEnabledChanged: _write("WirelessEnabled", wifiEnabled, v => wifiEnabled = v)
    onConnectivityCheckEnabledChanged: _write("ConnectivityCheckEnabled", connectivityCheckEnabled, v => connectivityCheckEnabled = v)
    function _write(prop, want, assign) {
        if (_syncing) return
        _syncing = true; assign(!want); _syncing = false
        NmCore.setBool(_nmPath, _nmIface, prop, want)
    }

    readonly property string _nmPath: "/org/freedesktop/NetworkManager"
    readonly property string _nmIface: "org.freedesktop.NetworkManager"
    readonly property string _devIface: "org.freedesktop.NetworkManager.Device"
    readonly property string _wifiIface: "org.freedesktop.NetworkManager.Device.Wireless"
    readonly property string _wiredIface: "org.freedesktop.NetworkManager.Device.Wired"
    readonly property string _apIface: "org.freedesktop.NetworkManager.AccessPoint"
    readonly property string _activeIface: "org.freedesktop.NetworkManager.Connection.Active"

    // D-Bus path -> {interface: properties}
    property var _objects: ({})
    property var _devices: ({})
    property var _settings: ({})
    property var _deviceOrder: []
    property Component _wifiComponent: Component { WifiDevice {} }
    property Component _wiredComponent: Component { WiredDevice {} }
    property Component _wifiNetComponent: Component { WifiNetwork {} }
    property Component _netComponent: Component { Network {} }
    property Component _settingsComponent: Component { NMSettings {} }

    property bool _queued: false
    function _schedule() {
        if (_queued) return
        _queued = true
        Qt.callLater(_sync)
    }

    function _utf8(b) {
        if (!b) return ""
        if (typeof b === "string") return b
        let out = "", i = 0
        while (i < b.length) {
            let c = b[i++]
            if (c >= 0xf0) c = ((c & 7) << 18) | ((b[i++] & 63) << 12) | ((b[i++] & 63) << 6) | (b[i++] & 63)
            else if (c >= 0xe0) c = ((c & 15) << 12) | ((b[i++] & 63) << 6) | (b[i++] & 63)
            else if (c >= 0xc0) c = ((c & 31) << 6) | (b[i++] & 63)
            out += String.fromCodePoint(c)
        }
        return out
    }

    // NMDeviceState -> ConnectionState (upstream translateState, Failed counts as Disconnecting).
    function _deviceState(s) {
        if (s === undefined || s <= 20) return ConnectionState.Unknown
        if (s === 30) return ConnectionState.Disconnected
        if (s < 100) return ConnectionState.Connecting
        if (s === 100) return ConnectionState.Connected
        return ConnectionState.Disconnecting
    }

    function _settingsSecurity(map) {
        const sec = map["802-11-wireless-security"]
        if (!sec || Object.keys(sec).length === 0) return WifiSecurityType.Open
        const key = sec["key-mgmt"] ?? "", proto = sec.proto ?? []
        const both = proto.includes("wpa") && proto.includes("rsn")
        switch (key) {
        case "none": return WifiSecurityType.StaticWep
        case "ieee8021x": return sec["auth-alg"] === "leap" ? WifiSecurityType.Leap : WifiSecurityType.DynamicWep
        case "wpa-psk": return both ? WifiSecurityType.WpaPsk : WifiSecurityType.Wpa2Psk
        case "wpa-eap": return both ? WifiSecurityType.WpaEap : WifiSecurityType.Wpa2Eap
        case "sae": return WifiSecurityType.Sae
        case "wpa-eap-suite-b-192": return WifiSecurityType.Wpa3SuiteB192
        case "owe": return WifiSecurityType.Owe
        }
        return WifiSecurityType.Open
    }

    // Settings objects of a device (AvailableConnections), loading new ones.
    function _deviceSettings(dp) {
        const out = []
        for (const p of dp.AvailableConnections ?? []) {
            let s = _settings[p]
            if (!s) {
                s = _settingsComponent.createObject(root, { dbusPath: p })
                _settings[p] = s
                NmCore.loadSettings(p)
            }
            if (s._loaded) out.push(s)
        }
        return out
    }

    function _reference(net, settings, activeConn) {
        if (settings.length === 0) return ""
        if (activeConn && settings.some(s => s.dbusPath === activeConn)) return activeConn
        let best = null
        for (const s of settings) if (!best || (s._map.connection?.timestamp ?? 0) > (best._map.connection?.timestamp ?? 0)) best = s
        return best.dbusPath
    }

    function _applyNetwork(net, settings, active) {
        const paths = settings.map(s => s.dbusPath)
        const activeConn = active?.props?.Connection
        const isActive = activeConn !== undefined && paths.includes(activeConn)
        net.known = settings.length > 0
        if (paths.join() !== Array.prototype.map.call(net.nmSettings, s => s.dbusPath).join()) net.nmSettings = settings
        net._referenceSettings = _reference(net, settings, isActive ? activeConn : "")
        net._activePath = isActive ? active.path : ""
        net.state = isActive ? (active.props.State ?? ConnectionState.Unknown) : ConnectionState.Disconnected
    }

    function _syncWifi(d, dp, active) {
        const w = _objects[d.dbusPath]?.[_wifiIface] ?? {}
        const mode = w.Mode ?? 0
        d.mode = [WifiDeviceMode.Unknown, WifiDeviceMode.AdHoc, WifiDeviceMode.Station, WifiDeviceMode.AccessPoint, WifiDeviceMode.Mesh][mode] ?? WifiDeviceMode.Unknown
        d._lastScan = (w.LastScan ?? -1) >= 0 ? w.LastScan : -1
        const caps = w.WirelessCapabilities ?? 0
        const groups = {}
        const group = ssid => groups[ssid] ?? (groups[ssid] = { aps: [], settings: [] })
        for (const path of w.AccessPoints ?? []) {
            const a = _objects[path]?.[_apIface]
            if (!a) continue
            const ssid = _utf8(a.Ssid)
            if (ssid && a.Mode === 2) group(ssid).aps.push({ path, a })
        }
        for (const s of _deviceSettings(dp)) {
            const m = s._map, wl = m["802-11-wireless"]
            if (!m.connection?.id || !m.connection?.uuid || !wl) continue
            const ssid = _utf8(wl.ssid)
            if (ssid && (!wl.mode || wl.mode === "infrastructure")) group(ssid).settings.push(s)
        }
        const visible = []
        for (const ssid of Object.keys(d._nets)) {
            if (groups[ssid]) continue
            d._nets[ssid].destroy()
            delete d._nets[ssid]
        }
        for (const ssid of Object.keys(groups)) {
            const g = groups[ssid]
            let net = d._nets[ssid]
            if (!net) {
                net = _wifiNetComponent.createObject(d, { name: ssid, device: d })
                d._nets[ssid] = net
            }
            _applyNetwork(net, g.settings, active)
            let ref = g.aps.find(x => x.path === w.ActiveAccessPoint) ?? null
            for (const x of g.aps) if (!ref || x.a.Strength > ref.a.Strength) ref = x
            net._referenceAp = ref?.path ?? ""
            net.signalStrength = ref ? ref.a.Strength / 100 : 0
            const refSettings = g.settings.find(s => s.dbusPath === net._referenceSettings)
            net.security = ref ? NmCore.security(caps, ref.a.Mode === 1, ref.a.Flags ?? 0, ref.a.WpaFlags ?? 0, ref.a.RsnFlags ?? 0)
                : refSettings ? _settingsSecurity(refSettings._map) : WifiSecurityType.Unknown
            if (d.scannerEnabled || net.state === ConnectionState.Connected || net.known) visible.push(ssid)
        }
        // Networks keep the order they appeared in.
        d._order = d._order.filter(s => visible.includes(s)).concat(visible.filter(s => !d._order.includes(s)))
        const values = d._order.map(s => d._nets[s])
        if (values.length !== d.networks.values.length || values.some((n, i) => n !== d.networks.values[i])) d.networks.values = values
    }

    function _syncWired(d, dp, active) {
        const wd = _objects[d.dbusPath]?.[_wiredIface] ?? {}
        d.linkSpeed = wd.Speed ?? 0
        d.hasLink = ((dp.InterfaceFlags ?? 0) & 0x10000) !== 0
        if (!d._net) d._net = _netComponent.createObject(d, { device: d })
        d._net.name = dp.Interface ?? ""
        _applyNetwork(d._net, _deviceSettings(dp), active)
        d.network = d.hasLink ? d._net : null
        const values = d.hasLink ? [d._net] : []
        if (values.length !== d.networks.values.length) d.networks.values = values
    }

    function _sync() {
        _queued = false
        for (const path of _deviceOrder) {
            const d = _devices[path], dp = _objects[path]?.[_devIface]
            if (!d || !dp) continue
            d._syncing = true
            d.name = dp.Interface ?? ""
            d.address = dp.HwAddress ?? ""
            d.state = _deviceState(dp.State)
            d.nmManaged = dp.Managed ?? false
            d.autoconnect = dp.Autoconnect ?? false
            d._syncing = false
            const ap = dp.ActiveConnection
            const active = ap && ap !== "/" && _objects[ap]?.[_activeIface] ? { path: ap, props: _objects[ap][_activeIface] } : null
            if (d.type === DeviceType.Wifi) _syncWifi(d, dp, active)
            else _syncWired(d, dp, active)
        }
        // Profiles no device offers any more.
        const used = new Set()
        for (const path of _deviceOrder) for (const p of _objects[path]?.[_devIface]?.AvailableConnections ?? []) used.add(p)
        for (const p of Object.keys(_settings)) {
            if (used.has(p)) continue
            _settings[p].destroy()
            delete _settings[p]
        }
        const list = _deviceOrder.map(p => _devices[p])
        if (list.length !== devices.values.length || list.some((d, i) => d !== devices.values[i])) devices.values = list
    }

    function _syncManager() {
        _syncing = true
        wifiEnabled = _nm.WirelessEnabled ?? false
        connectivityCheckEnabled = _nm.ConnectivityCheckEnabled ?? false
        _syncing = false
    }

    function _addDevice(path, props) {
        if (_devices[path]) return
        const type = props.DeviceType
        const component = type === 2 ? _wifiComponent : type === 1 ? _wiredComponent : null
        if (!component) return
        _devices[path] = component.createObject(root, { dbusPath: path })
        _deviceOrder.push(path)
    }

    function _removeDevice(path) {
        const d = _devices[path]
        if (!d) return
        delete _devices[path]
        _deviceOrder = _deviceOrder.filter(p => p !== path)
        devices.values = _deviceOrder.map(p => _devices[p])
        d.destroy()
    }

    property Connections _core: Connections {
        target: NmCore
        function onReady() { root._backend = NetworkBackendType.NetworkManager }
        function onInterfacesAdded(path, json) {
            const ifaces = JSON.parse(json)
            const obj = root._objects[path] ?? (root._objects[path] = {})
            for (const i in ifaces) obj[i] = ifaces[i]
            if (ifaces[root._nmIface]) { root._nm = obj[root._nmIface]; root._syncManager() }
            if (ifaces[root._devIface]) root._addDevice(path, ifaces[root._devIface])
            root._schedule()
        }
        function onInterfacesRemoved(path, ifaces) {
            const obj = root._objects[path]
            if (!obj) return
            for (const i of ifaces) delete obj[i]
            if (Object.keys(obj).length === 0) delete root._objects[path]
            if (ifaces.includes(root._devIface)) root._removeDevice(path)
            root._schedule()
        }
        function onPropertiesChanged(path, iface, json) {
            const obj = root._objects[path]
            if (!obj?.[iface]) return
            Object.assign(obj[iface], JSON.parse(json))
            if (iface === root._nmIface) { root._nm = Object.assign({}, obj[iface]); root._syncManager() }
            root._schedule()
        }
        function onStateChanged(path, iface, state, reason) {
            if (iface === root._devIface) {
                if (state === 120 && root._devices[path]) root._devices[path]._lastFailReason = reason
                return
            }
            // Connection.Active went down because the device did: tell the network why.
            if (reason !== 3) return
            for (const p of root._deviceOrder) {
                const d = root._devices[p]
                const nets = d.type === DeviceType.Wifi ? Object.values(d._nets) : (d._net ? [d._net] : [])
                const net = nets.find(n => n._activePath === path)
                if (!net) continue
                const why = { 7: ConnectionFailReason.NoSecrets, 8: ConnectionFailReason.WifiClientDisconnected,
                              10: ConnectionFailReason.WifiClientFailed, 11: ConnectionFailReason.WifiAuthTimeout,
                              53: ConnectionFailReason.WifiNetworkLost }[d._lastFailReason]
                if (why !== undefined) net.connectionFailed(why)
            }
        }
        function onSettingsLoaded(path, json) {
            const s = root._settings[path]
            if (!s) return
            s._map = JSON.parse(json)
            s._loaded = true
            root._schedule()
        }
        function onSettingsUpdated(path) { if (root._settings[path]) NmCore.loadSettings(path) }
        function onServiceLost() {
            for (const p of root._deviceOrder.slice()) root._removeDevice(p)
            root._objects = {}
            root._backend = NetworkBackendType.None
        }
    }
}
