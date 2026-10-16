#!/usr/bin/env python3
"""Fake BlueZ (org.bluez object manager, one adapter, two devices) on the bus in
DBUS_SYSTEM_BUS_ADDRESS, for tests/qml/bluetooth.qml."""
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

PROPS = "org.freedesktop.DBus.Properties"
OM = "org.freedesktop.DBus.ObjectManager"
AD = "org.bluez.Adapter1"
DEV = "org.bluez.Device1"
BAT = "org.bluez.Battery1"
objects = {}


class Obj(dbus.service.Object):
    def __init__(self, bus, path, ifaces):
        super().__init__(bus, path)
        self.path, self.ifaces = path, ifaces
        objects[path] = self

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, iface, name):
        return self.ifaces[iface][name]

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return self.ifaces[iface]

    @dbus.service.method(PROPS, in_signature="ssv")
    def Set(self, iface, name, value):
        self.update(iface, {name: value})

    @dbus.service.signal(PROPS, signature="sa{sv}as")
    def PropertiesChanged(self, iface, changed, invalidated):
        pass

    def update(self, iface, changed):
        self.ifaces[iface].update(changed)
        self.PropertiesChanged(iface, dbus.Dictionary(changed, signature="sv"), dbus.Array([], signature="s"))


class Adapter(Obj):
    @dbus.service.method(AD)
    def StartDiscovery(self):
        self.update(AD, {"Discovering": dbus.Boolean(True)})

    @dbus.service.method(AD)
    def StopDiscovery(self):
        self.update(AD, {"Discovering": dbus.Boolean(False)})

    @dbus.service.method(AD, in_signature="o")
    def RemoveDevice(self, path):
        obj = objects.pop(str(path))
        obj.remove_from_connection()
        root.InterfacesRemoved(path, dbus.Array(list(obj.ifaces), signature="s"))


class Device(Obj):
    @dbus.service.method(DEV)
    def Connect(self):
        if self.ifaces[DEV]["Blocked"]:
            raise dbus.exceptions.DBusException("blocked", name="org.bluez.Error.Failed")
        GLib.timeout_add(200, lambda: self.update(DEV, {"Connected": dbus.Boolean(True)}))

    @dbus.service.method(DEV)
    def Disconnect(self):
        self.update(DEV, {"Connected": dbus.Boolean(False)})

    @dbus.service.method(DEV)
    def Pair(self):
        self.update(DEV, {"Paired": dbus.Boolean(True), "Bonded": dbus.Boolean(True)})


class Root(dbus.service.Object):
    @dbus.service.method(OM, out_signature="a{oa{sa{sv}}}")
    def GetManagedObjects(self):
        return {dbus.ObjectPath(p): o.ifaces for p, o in objects.items()}

    @dbus.service.signal(OM, signature="oa{sa{sv}}")
    def InterfacesAdded(self, path, ifaces):
        pass

    @dbus.service.signal(OM, signature="oas")
    def InterfacesRemoved(self, path, ifaces):
        pass


class Mock(dbus.service.Object):
    @dbus.service.method("org.qsrs.Mock", in_signature="i")
    def Step(self, step):
        if step == 1:
            p = "/org/bluez/hci0/dev_33_33_33_33_33_33"
            d = Device(bus, p, {DEV: device("33:33:33:33:33:33", "Speaker", "audio-card", False)})
            root.InterfacesAdded(dbus.ObjectPath(p), d.ifaces)
        elif step == 2:
            adapter.update(AD, {"PowerState": "on-disabling"})


def device(address, name, icon, connected):
    return dbus.Dictionary({"Address": address, "Name": name, "Alias": name, "Icon": icon,
                            "Paired": dbus.Boolean(connected), "Bonded": dbus.Boolean(connected),
                            "Trusted": dbus.Boolean(connected), "Blocked": dbus.Boolean(False),
                            "WakeAllowed": dbus.Boolean(False), "Connected": dbus.Boolean(connected),
                            "Adapter": dbus.ObjectPath("/org/bluez/hci0")}, signature="sv")


DBusGMainLoop(set_as_default=True)
bus = dbus.SystemBus()
root = Root(bus, "/")
adapter = Adapter(bus, "/org/bluez/hci0", {AD: dbus.Dictionary({
    "Address": "00:11:22:33:44:55", "Alias": "test-adapter", "Powered": dbus.Boolean(True), "PowerState": "on",
    "Discoverable": dbus.Boolean(False), "DiscoverableTimeout": dbus.UInt32(180), "Discovering": dbus.Boolean(False),
    "Pairable": dbus.Boolean(True), "PairableTimeout": dbus.UInt32(0)}, signature="sv")})
Device(bus, "/org/bluez/hci0/dev_11_11_11_11_11_11", {
    DEV: device("11:11:11:11:11:11", "Headphones", "audio-headphones", True),
    BAT: dbus.Dictionary({"Percentage": dbus.Byte(80)}, signature="sv")})
Device(bus, "/org/bluez/hci0/dev_22_22_22_22_22_22", {DEV: device("22:22:22:22:22:22", "Mouse", "input-mouse", False)})
mock = Mock(bus, "/org/qsrs/Mock")
names = [dbus.service.BusName(n, bus) for n in ("org.bluez", "org.qsrs.Mock")]
GLib.MainLoop().run()
