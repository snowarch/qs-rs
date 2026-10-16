#!/usr/bin/env python3
"""Records the signals of the notification server under test (NotificationClosed,
ActionInvoked, NotificationReplied) for tests/qml/notifications.qml; Signals() returns them."""
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

seen = []


class Mock(dbus.service.Object):
    @dbus.service.method("org.qsrs.Mock", out_signature="as")
    def Signals(self):
        return seen


def record(*args, member=None):
    seen.append(" ".join([member] + [str(a) for a in args]))


DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
bus.add_signal_receiver(record, dbus_interface="org.freedesktop.Notifications", member_keyword="member")
mock = Mock(bus, "/org/qsrs/Mock")
name = dbus.service.BusName("org.qsrs.Mock", bus)
GLib.MainLoop().run()
