#!/usr/bin/env python3
"""Two StatusNotifierItems for tests/qml/systemtray.qml: "named" owns its own well-known name
and registers it; "pathonly" registers a bare path on the mock's connection (the libappindicator
way). Both register when org.kde.StatusNotifierWatcher appears. org.qsrs.Mock records the
calls items receive and lets the test change or drop them."""
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

SNI = "org.kde.StatusNotifierItem"
PROPS = "org.freedesktop.DBus.Properties"
calls = []


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

    @dbus.service.method(SNI, in_signature="ii")
    def Activate(self, x, y):
        calls.append(f"{self.props['Id']} Activate")

    @dbus.service.method(SNI, in_signature="ii")
    def SecondaryActivate(self, x, y):
        calls.append(f"{self.props['Id']} SecondaryActivate")

    @dbus.service.method(SNI, in_signature="is")
    def Scroll(self, delta, orientation):
        calls.append(f"{self.props['Id']} Scroll {delta} {orientation}")

    @dbus.service.signal(SNI)
    def NewTitle(self):
        pass

    @dbus.service.signal(SNI, signature="s")
    def NewStatus(self, status):
        pass


# 2x1 ARGB32 (network order): opaque red, half transparent blue
pixmap = dbus.Array([dbus.Struct((dbus.Int32(1), dbus.Int32(1), dbus.ByteArray(b"\xff\x00\x00\x00"))),
                     dbus.Struct((dbus.Int32(2), dbus.Int32(1), dbus.ByteArray(b"\xff\xff\x00\x00\x80\x00\x00\xff")))],
                    signature="(iiay)")


def base(id_, title):
    return {"Id": id_, "Title": title, "Status": "Active", "Category": "Communications", "IconName": "",
            "IconThemePath": "", "IconPixmap": dbus.Array([], signature="(iiay)"), "AttentionIconName": "",
            "AttentionIconPixmap": dbus.Array([], signature="(iiay)"), "OverlayIconName": "",
            "OverlayIconPixmap": dbus.Array([], signature="(iiay)"),
            "ToolTip": dbus.Struct(("", dbus.Array([], signature="(iiay)"), "Tip", "Tip text"), signature="sa(iiay)ss"),
            "Menu": dbus.ObjectPath("/NO_DBUSMENU"), "ItemIsMenu": dbus.Boolean(False)}


DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
named_conn = dbus.SessionBus(private=True)
named_name = "org.kde.StatusNotifierItem-4242-1"
named_conn.request_name(named_name)
named = Item(named_conn, "/StatusNotifierItem", {**base("named", "Named"), "IconName": "folder"})
pathonly = Item(bus, "/org/ayatana/NotificationItem/pathonly",
                {**base("pathonly", "Path only"), "IconPixmap": pixmap, "Menu": dbus.ObjectPath("/menu")})


def register(owner):
    if not owner:
        return
    w = bus.get_object("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher")
    w.RegisterStatusNotifierItem(named_name, dbus_interface="org.kde.StatusNotifierWatcher")
    w.RegisterStatusNotifierItem("/org/ayatana/NotificationItem/pathonly", dbus_interface="org.kde.StatusNotifierWatcher")


class Mock(dbus.service.Object):
    @dbus.service.method("org.qsrs.Mock", out_signature="as")
    def Calls(self):
        return calls

    @dbus.service.method("org.qsrs.Mock", in_signature="ss")
    def Change(self, title, status):
        pathonly.props["Title"] = title
        pathonly.props["Status"] = status
        pathonly.NewTitle()
        pathonly.NewStatus(status)

    @dbus.service.method("org.qsrs.Mock")
    def DropNamed(self):
        named_conn.release_name(named_name)


bus.watch_name_owner("org.kde.StatusNotifierWatcher", register)
mock = Mock(bus, "/org/qsrs/Mock")
name = dbus.service.BusName("org.qsrs.Mock", bus)
GLib.MainLoop().run()
