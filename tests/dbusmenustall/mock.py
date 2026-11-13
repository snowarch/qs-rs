#!/usr/bin/env python3
"""Two tray items with a dbusmenu each, for tests/qml/dbusmenustall.qml. The menu of "stuck" never answers a
property read (shelly-notifications does that for IconThemePath: the call never returns); "normal" answers
everything. org.qsrs.Mock only tells the wrapper the mock is up."""
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

SNI = "org.kde.StatusNotifierItem"
PROPS = "org.freedesktop.DBus.Properties"
MENU = "com.canonical.dbusmenu"


class Item(dbus.service.Object):
    def __init__(self, conn, path, props):
        super().__init__(conn, path)
        self.props = props

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return dbus.Dictionary(self.props, signature="sv")

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, iface, name):
        return self.props[name]


def base(id_, menu):
    empty = dbus.Array([], signature="(iiay)")
    return {"Id": id_, "Title": id_, "Status": "Active", "Category": "ApplicationStatus", "IconName": "",
            "IconThemePath": "", "IconPixmap": empty, "AttentionIconName": "", "AttentionIconPixmap": empty,
            "OverlayIconName": "", "OverlayIconPixmap": empty,
            "ToolTip": dbus.Struct(("", empty, "", ""), signature="sa(iiay)ss"),
            "Menu": dbus.ObjectPath(menu), "ItemIsMenu": dbus.Boolean(False)}


class Menu(dbus.service.Object):
    def __init__(self, conn, path, label):
        super().__init__(conn, path)
        self.label = label

    @dbus.service.method(MENU, in_signature="iias", out_signature="u(ia{sv}av)")
    def GetLayout(self, parent, depth, names):
        entry = dbus.Struct((dbus.Int32(1), dbus.Dictionary({"label": self.label}, signature="sv"),
                             dbus.Array([], signature="v")), signature="ia{sv}av")
        root = (dbus.Int32(0), dbus.Dictionary({"children-display": "submenu"}, signature="sv"),
                dbus.Array([entry], signature="v"))
        return dbus.UInt32(1), dbus.Struct(root, signature="ia{sv}av")

    @dbus.service.method(MENU, in_signature="i", out_signature="b")
    def AboutToShow(self, id_):
        return False

    @dbus.service.method(MENU, in_signature="isvu")
    def Event(self, id_, event, data, timestamp):
        pass


class StuckMenu(Menu):
    # Replaces the object's own Properties.Get: the reply callback is never called.
    @dbus.service.method(PROPS, in_signature="ss", out_signature="v", async_callbacks=("reply", "error"))
    def Get(self, iface, name, reply, error):
        pass


DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
items = [("stuck", "/menu_stuck"), ("normal", "/menu_normal")]
objs = [Item(bus, f"/org/ayatana/NotificationItem/{i}", base(i, m)) for i, m in items]
menus = [StuckMenu(bus, "/menu_stuck", "Stuck entry"), Menu(bus, "/menu_normal", "Normal entry")]


def register(owner):
    if not owner:
        return
    w = bus.get_object("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher")
    for i, _ in items:
        w.RegisterStatusNotifierItem(f"/org/ayatana/NotificationItem/{i}", dbus_interface="org.kde.StatusNotifierWatcher")


class Mock(dbus.service.Object):
    pass


bus.watch_name_owner("org.kde.StatusNotifierWatcher", register)
mock = Mock(bus, "/org/qsrs/Mock")
name = dbus.service.BusName("org.qsrs.Mock", bus)
GLib.MainLoop().run()
