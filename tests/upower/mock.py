#!/usr/bin/env python3
"""Fake UPower and power-profiles-daemon on the bus in DBUS_SYSTEM_BUS_ADDRESS, for
tests/qml/upower.qml. `org.qsrs.Mock.Step` changes state the way the real daemons would."""
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

PROPS = "org.freedesktop.DBus.Properties"
UP = "org.freedesktop.UPower"
DEV = "org.freedesktop.UPower.Device"
PP = "org.freedesktop.UPower.PowerProfiles"


class Obj(dbus.service.Object):
    def __init__(self, bus, path, iface, props):
        super().__init__(bus, path)
        self.iface, self.props = iface, props

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, iface, name):
        return self.props[name]

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        if iface != self.iface:
            raise dbus.exceptions.DBusException("no such interface", name="org.freedesktop.DBus.Error.UnknownInterface")
        return self.props

    @dbus.service.method(PROPS, in_signature="ssv")
    def Set(self, iface, name, value):
        self.update({name: value})

    @dbus.service.signal(PROPS, signature="sa{sv}as")
    def PropertiesChanged(self, iface, changed, invalidated):
        pass

    def update(self, changed):
        self.props.update(changed)
        self.PropertiesChanged(self.iface, dbus.Dictionary(changed, signature="sv"), dbus.Array([], signature="s"))


def device(kind, percentage, state, model):
    return {"Type": dbus.UInt32(kind), "PowerSupply": dbus.Boolean(kind == 2), "Energy": dbus.Double(30.0),
            "EnergyFull": dbus.Double(50.0), "EnergyRate": dbus.Double(7.5), "TimeToEmpty": dbus.Int64(3600),
            "TimeToFull": dbus.Int64(0), "Percentage": dbus.Double(percentage), "IsPresent": dbus.Boolean(True),
            "State": dbus.UInt32(state), "Capacity": dbus.Double(91.0), "IconName": "battery-good-symbolic",
            "NativePath": "BAT0" if kind == 2 else "", "Model": model}


class UPower(Obj):
    def __init__(self, bus):
        super().__init__(bus, "/org/freedesktop/UPower", UP, {"OnBattery": dbus.Boolean(False), "DaemonVersion": "1.0"})
        self.devices = ["/org/freedesktop/UPower/devices/battery_BAT0"]

    @dbus.service.method(UP, out_signature="ao")
    def EnumerateDevices(self):
        return dbus.Array(self.devices, signature="o")

    @dbus.service.method(UP, out_signature="o")
    def GetDisplayDevice(self):
        return dbus.ObjectPath("/org/freedesktop/UPower/devices/DisplayDevice")

    @dbus.service.signal(UP, signature="o")
    def DeviceAdded(self, path):
        pass

    @dbus.service.signal(UP, signature="o")
    def DeviceRemoved(self, path):
        pass


class Mock(dbus.service.Object):
    @dbus.service.method("org.qsrs.Mock", in_signature="i")
    def Step(self, step):
        if step == 1:
            upower.update({"OnBattery": dbus.Boolean(True)})
            battery.update({"Percentage": dbus.Double(40.0), "State": dbus.UInt32(2)})
        elif step == 2:
            mouse_path = "/org/freedesktop/UPower/devices/mouse_0"
            global mouse
            mouse = Obj(bus, mouse_path, DEV, device(5, 80.0, 2, "Mouse"))
            upower.devices.append(mouse_path)
            upower.DeviceAdded(dbus.ObjectPath(mouse_path))
        elif step == 3:
            upower.devices.remove(battery_path)
            battery.remove_from_connection()
            upower.DeviceRemoved(dbus.ObjectPath(battery_path))
        elif step == 4:
            profiles.update({"PerformanceDegraded": "high-operating-temperature"})


DBusGMainLoop(set_as_default=True)
bus = dbus.SystemBus()
upower = UPower(bus)
battery_path = "/org/freedesktop/UPower/devices/battery_BAT0"
battery = Obj(bus, battery_path, DEV, device(2, 75.0, 1, "Test Battery"))
display = Obj(bus, "/org/freedesktop/UPower/devices/DisplayDevice", DEV, device(2, 75.0, 1, ""))
hold = dbus.Dictionary({"Profile": "power-saver", "ApplicationId": "test", "Reason": "testing"}, signature="sv")
profiles = Obj(bus, "/org/freedesktop/UPower/PowerProfiles", PP, {
    "ActiveProfile": "balanced",
    "Profiles": dbus.Array([dbus.Dictionary({"Profile": p, "Driver": "mock"}, signature="sv")
                            for p in ("power-saver", "balanced", "performance")], signature="a{sv}"),
    "PerformanceDegraded": "",
    "ActiveProfileHolds": dbus.Array([hold], signature="a{sv}"),
})
mock = Mock(bus, "/org/qsrs/Mock")
names = [dbus.service.BusName(n, bus) for n in (UP, PP, "org.qsrs.Mock")]
GLib.MainLoop().run()
