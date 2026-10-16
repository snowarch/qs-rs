#!/usr/bin/env python3
"""Fake MPRIS player (org.mpris.MediaPlayer2.qsrs) on the private session bus, for
tests/qml/mpris.qml. Records calls so the test can check what reached the player."""
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

PROPS = "org.freedesktop.DBus.Properties"
APP = "org.mpris.MediaPlayer2"
PL = "org.mpris.MediaPlayer2.Player"
calls = []


def meta(n):
    return dbus.Dictionary({"mpris:trackid": dbus.ObjectPath(f"/track/{n}"), "xesam:title": f"Song {n}",
                            "xesam:artist": dbus.Array(["A", "B"], signature="s"), "xesam:album": "Album",
                            "mpris:length": dbus.Int64(200_000_000), "mpris:artUrl": "file:///art.png"}, signature="sv")


class Player(dbus.service.Object):
    def __init__(self, bus):
        super().__init__(bus, "/org/mpris/MediaPlayer2")
        self.ifaces = {
            APP: {"Identity": "Mock Player", "DesktopEntry": "mock", "CanQuit": dbus.Boolean(True),
                  "CanRaise": dbus.Boolean(False), "CanSetFullscreen": dbus.Boolean(False), "Fullscreen": dbus.Boolean(False),
                  "SupportedUriSchemes": dbus.Array(["file"], signature="s"), "SupportedMimeTypes": dbus.Array([], signature="s")},
            PL: {"CanControl": dbus.Boolean(True), "CanPlay": dbus.Boolean(True), "CanPause": dbus.Boolean(True),
                 "CanSeek": dbus.Boolean(True), "CanGoNext": dbus.Boolean(True), "CanGoPrevious": dbus.Boolean(False),
                 "PlaybackStatus": "Paused", "LoopStatus": "None", "Rate": dbus.Double(1.0), "MinimumRate": dbus.Double(0.5),
                 "MaximumRate": dbus.Double(2.0), "Shuffle": dbus.Boolean(False), "Volume": dbus.Double(0.5),
                 "Position": dbus.Int64(10_000_000), "Metadata": meta(1)},
        }
        self.track = 1

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, iface, name):
        return self.ifaces[iface][name]

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return self.ifaces[iface]

    @dbus.service.method(PROPS, in_signature="ssv")
    def Set(self, iface, name, value):
        calls.append(f"Set {name}={value}")
        self.update(iface, {name: value})

    @dbus.service.signal(PROPS, signature="sa{sv}as")
    def PropertiesChanged(self, iface, changed, invalidated):
        pass

    def update(self, iface, changed):
        self.ifaces[iface].update(changed)
        quiet = {k: v for k, v in changed.items() if k != "Position"}
        if quiet:
            self.PropertiesChanged(iface, dbus.Dictionary(quiet, signature="sv"), dbus.Array([], signature="s"))

    @dbus.service.signal(PL, signature="x")
    def Seeked(self, position):
        pass

    @dbus.service.method(PL)
    def Play(self):
        calls.append("Play")
        self.update(PL, {"PlaybackStatus": "Playing"})

    @dbus.service.method(PL)
    def Pause(self):
        calls.append("Pause")
        self.update(PL, {"PlaybackStatus": "Paused"})

    @dbus.service.method(PL)
    def Next(self):
        calls.append("Next")
        self.track += 1
        self.update(PL, {"Metadata": meta(self.track), "Position": dbus.Int64(0)})

    @dbus.service.method(PL, in_signature="ox")
    def SetPosition(self, track, position):
        calls.append(f"SetPosition {track} {position}")
        self.update(PL, {"Position": dbus.Int64(position)})
        self.Seeked(dbus.Int64(position))

    @dbus.service.method(APP)
    def Quit(self):
        calls.append("Quit")
        GLib.timeout_add(50, lambda: (player_name.get_bus().release_name(player_name.get_name()), False)[1])


class Mock(dbus.service.Object):
    @dbus.service.method("org.qsrs.Mock", out_signature="s")
    def Calls(self):
        return ";".join(calls)


DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
player = Player(bus)
mock = Mock(bus, "/org/qsrs/Mock")
player_name = dbus.service.BusName("org.mpris.MediaPlayer2.qsrs", bus)
mock_name = dbus.service.BusName("org.qsrs.Mock", bus)
GLib.MainLoop().run()
