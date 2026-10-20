#!/usr/bin/env python3
"""Fake polkitd and logind on the bus in DBUS_SYSTEM_BUS_ADDRESS plus a fake agent helper on
QSRS_POLKIT_HELPER_SOCKET, for tests/qml/polkit.qml. The helper speaks polkit 127's socket
protocol: reads user and cookie, sends an escaped info line and a password prompt, answers
SUCCESS for "right" and PAM_ERROR_MSG + FAILURE otherwise."""
import json
import os
import socket
import threading

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

AUTH = "org.freedesktop.PolicyKit1.Authority"
AGENT = "org.freedesktop.PolicyKit1.AuthenticationAgent"
LOGIN = "org.freedesktop.login1"

state = {"registered": [], "unregistered": 0, "results": {}, "conversations": [], "intruder": ""}
agent = {}
lock = threading.Lock()


def identities(mode):
    uid = os.getuid()
    user = lambda u: dbus.Struct(("unix-user", dbus.Dictionary({"uid": dbus.UInt32(u)}, signature="sv")))
    if mode == "netgroup":
        return dbus.Array([dbus.Struct(("unix-netgroup", dbus.Dictionary({"name": "ng"}, signature="sv")))], signature="(sa{sv})")
    # A duplicate the agent must drop, root as a second user, and a group.
    group = dbus.Struct(("unix-group", dbus.Dictionary({"gid": dbus.UInt32(0)}, signature="sv")))
    return dbus.Array([user(uid), user(uid), user(0), group], signature="(sa{sv})")


class Authority(dbus.service.Object):
    @dbus.service.method(AUTH, in_signature="(sa{sv})ss", sender_keyword="sender")
    def RegisterAuthenticationAgent(self, subject, locale, path, sender=None):
        # QSRS_MOCK_POLKIT_BUSY=N: another agent holds the session for the first N attempts.
        state["attempts"] = state.get("attempts", 0) + 1
        if state["attempts"] <= int(os.environ.get("QSRS_MOCK_POLKIT_BUSY", "0")):
            raise dbus.exceptions.DBusException("An authentication agent already exists for the given subject", name="org.freedesktop.PolicyKit1.Error.Failed")
        agent.update(sender=sender, path=path)
        state["registered"].append({"kind": str(subject[0]), "session": str(subject[1].get("session-id", "")), "locale": str(locale), "path": str(path)})

    @dbus.service.method(AUTH, in_signature="(sa{sv})s")
    def UnregisterAuthenticationAgent(self, subject, path):
        state["unregistered"] += 1


class Manager(dbus.service.Object):
    @dbus.service.method(LOGIN + ".Manager", in_signature="u", out_signature="o")
    def GetSessionByPID(self, pid):
        return "/org/freedesktop/login1/session/c7"


class Session(dbus.service.Object):
    @dbus.service.method("org.freedesktop.DBus.Properties", in_signature="ss", out_signature="v")
    def Get(self, iface, name):
        return {"Id": "c7"}[name]

    @dbus.service.method("org.freedesktop.DBus.Properties", in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return {"Id": "c7"}


class Mock(dbus.service.Object):
    def __init__(self, bus, path):
        super().__init__(bus, path)
        self.bus = bus

    def proxy(self, bus=None):
        return (bus or self.bus).get_object(agent["sender"], agent["path"])

    @dbus.service.method("org.qsrs.Mock", in_signature="ss")
    def Begin(self, cookie, mode):
        state["results"][cookie] = "pending"

        def done(*_):
            state["results"][cookie] = "ok"

        def failed(e):
            state["results"][cookie] = e.get_dbus_name() + ": " + e.get_dbus_message()

        self.proxy().BeginAuthentication(
            "org.qsrs.test." + cookie, "Authenticate " + cookie, "dialog-password",
            dbus.Dictionary({}, signature="ss"), cookie, identities(mode),
            dbus_interface=AGENT, reply_handler=done, error_handler=failed, timeout=60)

    @dbus.service.method("org.qsrs.Mock", in_signature="s")
    def Cancel(self, cookie):
        self.proxy().CancelAuthentication(cookie, dbus_interface=AGENT)

    @dbus.service.method("org.qsrs.Mock")
    def Intrude(self):
        # A second connection is not polkitd: the agent must refuse it.
        other = dbus.bus.BusConnection(os.environ["DBUS_SYSTEM_BUS_ADDRESS"])

        def done(*_):
            state["intruder"] = "accepted"

        def failed(e):
            state["intruder"] = e.get_dbus_name()

        self.proxy(other).BeginAuthentication(
            "org.qsrs.evil", "evil", "", dbus.Dictionary({}, signature="ss"), "evil", identities("users"),
            dbus_interface=AGENT, reply_handler=done, error_handler=failed, timeout=60)
        self.other = other

    @dbus.service.method("org.qsrs.Mock", out_signature="s")
    def State(self):
        with lock:
            return json.dumps(state)


def converse(conn):
    f = conn.makefile("rwb", buffering=0)
    user = f.readline().decode().rstrip("\n")
    cookie = f.readline().decode().rstrip("\n")
    conv = {"user": user, "cookie": cookie, "responses": [], "closed": False}
    with lock:
        state["conversations"].append(conv)
    # "Contraseña" with g_strescape's octal bytes.
    f.write(b"PAM_TEXT_INFO Contrase\\303\\261a para " + user.encode() + b"\n")
    f.write(b"PAM_PROMPT_ECHO_OFF Password: \n")
    line = f.readline()
    if not line:
        with lock:
            conv["closed"] = True
        return
    response = line.decode().rstrip("\n")
    with lock:
        conv["responses"].append(response)
    if response == "right":
        f.write(b"SUCCESS\n")
    else:
        f.write(b"PAM_ERROR_MSG Wrong password\nFAILURE\n")
    conn.close()


def serve(path):
    if os.path.exists(path):
        os.unlink(path)
    s = socket.socket(socket.AF_UNIX)
    s.bind(path)
    s.listen()
    while True:
        conn, _ = s.accept()
        threading.Thread(target=converse, args=(conn,), daemon=True).start()


DBusGMainLoop(set_as_default=True)
bus = dbus.SystemBus()
Authority(bus, "/org/freedesktop/PolicyKit1/Authority")
Manager(bus, "/org/freedesktop/login1")
Session(bus, "/org/freedesktop/login1/session/c7")
Mock(bus, "/org/qsrs/Mock")
threading.Thread(target=serve, args=(os.environ["QSRS_POLKIT_HELPER_SOCKET"],), daemon=True).start()
names = [dbus.service.BusName(n, bus) for n in ("org.freedesktop.PolicyKit1", LOGIN, "org.qsrs.Mock")]
GLib.MainLoop().run()
