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

iNiR still runs on stock Quickshell today; qs-rs is where it's headed.

## Numbers

iNiR on both runtimes, same config, each in its own headless KWin (2026-10-27, 3 runs each):

| | Quickshell 0.3.2 | qs-rs | |
|---|---:|---:|---:|
| first frame | 2.6 s | **0.87 s** | ~3x faster |
| first frame, empty QML cache | 2.6 s | 2.5 s | same |
| CPU in the first 30 s | 7.9 s | **5.3 s** | -33% |
| threads | 90 | **45** | -50% |
| RSS | 632 MB | **558 MB** | -12% |
| PSS | 494 MB | **436 MB** | -12% |
| VRAM | 219 MiB | **199 MiB** | -9% |

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

## Build

```sh
cargo build --release      # Qt 6, LayerShellQt and a C++ compiler
scripts/test.sh            # every tests/qml/*.qml, isolated HOME and XDG dirs
```

iNiR's launcher picks it up from `INIR_QS_BIN` and falls back to Quickshell when it's missing.

## License and credits

GNU LGPL 3, same as Quickshell: [LICENSE](LICENSE), on top of the GPL in [LICENSE-GPL](LICENSE-GPL). Semantics ported
from [Quickshell](https://git.outfoxxed.me/quickshell/quickshell) by outfoxxed and contributors; any bug in the port
is ours.

This code was written by: Hikari.
