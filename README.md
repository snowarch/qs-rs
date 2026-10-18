# qs-rs

Quickshell, rewritten in Rust, for exactly one shell: [iNiR](https://github.com/snowarch/inir).

Same QML API as Quickshell 0.3.2, same CLI, IPC socket and log files, and its process is named
`qs`, so inir's launcher, niri binds and `qs ipc call` don't notice the swap. They just get
lighter.

## Why one shell

General runtimes pay for every config they might meet. qs-rs only meets inir, so both sides bend
toward each other: when a change in inir makes the pair faster, inir changes too. Upstream
parity is the floor, not the ceiling.

Rust owns all state and I/O (processes, files, D-Bus, PipeWire, Wayland protocols) off the GUI
thread; Qt only draws. One D-Bus thread per bus, one poll thread for every child process, one
inotify thread for every file watcher, one GL context for every window.

## Numbers

inir on both runtimes, same config and env, each in its own headless KWin, warm QML cache,
sampled 20 s after start and after a minute idle (mean of the warm runs, 2026-10-17):

| | Quickshell 0.3.2 | qs-rs | |
|---|---:|---:|---:|
| load time | 4111 ms | **657 ms** | 6.3x faster |
| load time, first start after an update | 4252 ms | **1943 ms** | 2.2x faster |
| CPU spent starting | 7.5 s | **2.8 s** | -62% |
| CPU over 60 s idle | 785 ms | **665 ms** | -15% |
| threads | 89 | **40** | -55% |
| RSS | 568 MB | **479 MB** | -16% |
| peak RSS | 597 MB | **509 MB** | -15% |
| PSS | 445 MB | **369 MB** | -17% |
| heap (anon) | 381 MB | **288 MB** | -24% |
| VRAM | 163 MiB | **146 MiB** | -10% |

Quickshell never keeps a QML cache for a shell, so every start compiles it; qs-rs recompiles only
when an update changes a type the shell uses. Opening inir's control panel peaks at 806 MB on
Quickshell and 513 MB on qs-rs (Mesa's optimized shader variants, compiled at every start, are
skipped). Reloading keeps the shown windows instead of recreating them.

Same pixels on screen: 191 surfaces of inir's three families shot on both. Ryzen 5000 APU (Vega), Linux 7.2,
Qt 6.12, Mesa 26.2. A single benchmark is never the whole truth, but this one is pretty loud.

## Status

inir runs on it as a full daily session: bar, dock, panels, notifications, tray and menus,
PipeWire, MPRIS, UPower, Bluetooth, NetworkManager, Polkit agent, PAM lock screen, session lock,
toplevels, idle monitor, IPC, hot reload. Animations step at the screen's refresh rate (one GL
context for every window, so no per-window render threads). Still missing: `ScreencopyView`
pixels (the type loads, capture is not ported) and `IdleInhibitor`.
Shells other than inir may trip over types nobody asked for yet.

## Build

```sh
cargo build --release      # Qt 6, LayerShellQt and a C++ compiler
scripts/test.sh            # every tests/qml/*.qml, isolated HOME and XDG dirs
```

## License

GNU LGPL 3, same as Quickshell: [LICENSE](LICENSE), on top of the GPL in
[LICENSE-GPL](LICENSE-GPL).

## Credits

Semantics ported from [Quickshell](https://git.outfoxxed.me/quickshell/quickshell) by outfoxxed
and contributors; any bug in the port is ours.

This code was written by: Hikari.
