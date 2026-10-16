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
sampled 20 s after start (mean of the warm runs):

| | Quickshell 0.3.2 | qs-rs | |
|---|---:|---:|---:|
| load time | 4042 ms | **632 ms** | 6.4x faster |
| CPU spent starting | 7.6 s | **4.0 s** | -47% |
| threads | 94 | **40** | -58% |
| RSS | 567 MB | **472 MB** | -17% |
| PSS | 454 MB | **375 MB** | -18% |
| heap (anon) | 389 MB | **287 MB** | -26% |
| VRAM | 164 MiB | **148 MiB** | -10% |

Same pixels on screen. Ryzen 5000 APU (Vega), Linux 7.2,
Qt 6.12, Mesa 26.2. A single benchmark is never the whole truth, but this one is pretty loud.

## Status

inir runs on it as a full daily session: bar, dock, panels, notifications, tray and menus,
PipeWire, MPRIS, UPower, Bluetooth, NetworkManager, Polkit agent, PAM lock screen, session lock,
toplevels, idle monitor, IPC. Still missing: `ScreencopyView`, `IdleInhibitor` and hot reload.
Shells other than inir may trip over types nobody asked for yet.

## Build

```sh
cargo build --release      # Qt 6, LayerShellQt and a C++ compiler
scripts/test.sh            # every tests/qml/*.qml, isolated HOME and XDG dirs
```

## Credits

Semantics ported from [Quickshell](https://git.outfoxxed.me/quickshell/quickshell) by outfoxxed
and contributors; any bug in the port is ours.

This code was written by: Hikari.
