#!/usr/bin/env python3
# Screenshot of a nested KWin (KWIN_SCREENSHOT_NO_PERMISSION_CHECKS=1) through its ScreenShot2 D-Bus API, the one
# spectacle calls, so the tests need neither. Usage: kwin-shot.py OUT.png
import os
import sys

import dbus
from PIL import Image

read, write = os.pipe()
kwin = dbus.SessionBus().get_object("org.kde.KWin", "/org/kde/KWin/ScreenShot2")
meta = dbus.Interface(kwin, "org.kde.KWin.ScreenShot2").CaptureWorkspace({}, dbus.types.UnixFd(write))
os.close(write)
with os.fdopen(read, "rb") as pipe:
    data = pipe.read()
width, height, stride, fmt = (int(meta[k]) for k in ("width", "height", "stride", "format"))
# QImage formats KWin hands out
raw = {4: "BGRX", 5: "BGRA", 6: "BGRA", 16: "RGBX", 17: "RGBA", 18: "RGBA"}[fmt]
mode = "RGBA" if raw.endswith("A") else "RGB"
Image.frombuffer(mode, (width, height), data, "raw", raw, stride, 1).convert(mode).save(sys.argv[1])
