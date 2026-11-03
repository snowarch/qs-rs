# qs-rs

Quickshell, rewritten in Rust, for exactly one shell: [iNiR](https://inir.space). Same QML API as Quickshell 0.3.2,
same CLI and IPC, and the process is still called `qs`, so iNiR doesn't notice the swap. It just gets faster.

Written by Hikari, an AI agent; the maintainer steers by what shows up on screen and doesn't read the code. The rules
and plans behind it stay on the maintainer's machine, on purpose.

## Where it sits

iNiR is a few pieces, each with one job:

- [iNiR](https://github.com/snowarch/iNiR): the shell itself, in QML.
- **qs-rs**: runs that QML, in place of Quickshell.
- [inir-accel](https://github.com/snowarch/inir-accel): what the shell runs in the background (colours, thumbnails,
  sensors), in Rust.
- [inir-cli](https://github.com/snowarch/inir-cli): the `inir` command you type.
- [inir-hub](https://github.com/snowarch/inir-hub): the families, themes and widgets you install, served at
  [inir.space/hub](https://inir.space/hub).

iNiR v2.33 runs on stock Quickshell. Its next update installs qs-rs prebuilt, with its own Qt, and falls back to
Quickshell on a machine where qs-rs doesn't start.

## Numbers

iNiR on both runtimes, same config, each in its own headless KWin (2026-11-03, 3 runs each):

| | Quickshell 0.3.2 | qs-rs | |
|---|---:|---:|---:|
| first frame | 2.3 s | **0.74 s** | ~3x faster |
| first frame, empty QML cache | 2.8 s | **2.5 s** | -10% |
| CPU in the first 30 s | 5.5 s | **3.3 s** | -40% |
| threads | 86 | **44** | -49% |
| RSS | 560 MB | **474 MB** | -15% |
| PSS | 443 MB | **413 MB** | -7% |
| VRAM | 246 MiB | **220 MiB** | -11% |

Quickshell keeps no QML cache for a shell, so every start is its cold start. Ryzen 7 5700G (Vega), Linux 7.3,
Qt 6.12, Mesa 26.2.

## What it adds

- `qs errors`: every warning of the running shell, grouped by where it comes from (`--static` compiles every file
  first, `-f` keeps watching). `qs check` compiles the whole config without running it.
- `qs memory`, `qs frames`, `qs ui`: where the memory sits, what each frame costs and why, and what a person would read
  on screen.
- Live window previews: `ScreencopyView { niriWindowId: id }` draws one window from niri's ScreenCast as GPU textures
  (dmabuf, no copy). `sourceRect` crops it for free, and `sourceRectBase` keeps the crop in place when the window
  resizes.
- WebEngine (`hasVersion(0, 3, ["webengine"])`), which iNiR's sidebar web apps run on.
- Hot reload that keeps windows open, `watchFiles` (save to reload in 0.16 s), and on a crash the native backtrace plus
  the QML stack of every engine, `file.qml:line` included.

## Status

Everything iNiR uses works, all three families, as a daily session. Missing, because iNiR doesn't use them:
`IdleInhibitor`, `DataStream`, `SocketServer`, the widget wrappers; Hyprland is IPC only. Out of scope: I3/Sway,
Greetd, X11.

## Get it

Every `v*` tag builds a bundle on CI: the binary plus the Qt 6.12 it was built against, so it runs the same on every
distro ([Releases](https://github.com/snowarch/qs-rs/releases)). iNiR's setup and update install the one its base pins
by sha256; nothing compiles on your machine. iNiR's launcher picks it up from `INIR_QS_BIN` and falls back to
Quickshell when it's missing.

To build it yourself:

```sh
cargo build --release      # Qt 6, LayerShellQt and a C++ compiler
scripts/test.sh            # every tests/qml/*.qml, isolated HOME and XDG dirs
```

## License and credits

GNU LGPL 3, same as Quickshell: [LICENSE](LICENSE), on top of the GPL in [LICENSE-GPL](LICENSE-GPL). Semantics ported
from [Quickshell](https://git.outfoxxed.me/quickshell/quickshell) by outfoxxed and contributors; any bug in the port
is ours.

This code was written by: Hikari.
