# qs-rs

Quickshell, rewritten in Rust, for exactly one shell: [iNiR](https://github.com/snowarch/inir).
Same QML API as Quickshell 0.3.2, same CLI and IPC, and the process is still called `qs`, so
inir doesn't notice the swap. It just gets lighter.

> [!WARNING]
> **Written by an AI, on autopilot.** Every line here was written autonomously by Hikari, a
> Claude Code agent (Claude Opus 5.5, `claude-opus-5-5`, extended thinking, effort set per
> session). The maintainer steers by what shows up on screen and doesn't read the code. That
> makes qs-rs an experiment too: how far an AI can take a project like this almost on its own.
>
> The development rules, plans and agent instructions behind it stay on the maintainer's machine
> and are never published. That is on purpose.

## At a glance

inir on both runtimes, same config, each in its own headless KWin (mean of 3 runs, 2026-10-17):

```
                       Quickshell 0.3.2      qs-rs
  load time                     4100 ms     632 ms   6.5x faster
  CPU to start                    8.2 s      3.1 s   -62%
  threads                            88         38   -57%
  memory (RSS)                   559 MB     477 MB   -15%
  control panel peak             806 MB     513 MB   -36%
```

<details>
<summary>Every number</summary>

| | Quickshell 0.3.2 | qs-rs | |
|---|---:|---:|---:|
| load time | 4100 ms | **632 ms** | 6.5x faster |
| load time, empty QML cache | 4228 ms | **2520 ms** | 1.7x faster |
| CPU spent starting | 8.2 s | **3.1 s** | -62% |
| CPU over 60 s idle | 817 ms | **680 ms** | -17% |
| threads | 88 | **38** | -57% |
| RSS | 559 MB | **477 MB** | -15% |
| peak RSS | 630 MB | **551 MB** | -13% |
| PSS | 434 MB | **364 MB** | -16% |
| heap (anon) | 375 MB | **285 MB** | -24% |
| VRAM | 160 MiB | **145 MiB** | -9% |
| first frame of a panel | 8 ms | **3 ms** | |

Warm cache, sampled 20 s after start and after a minute idle. Quickshell never keeps a QML cache
for a shell, so every start is its cold start. Same pixels on screen: 191 surfaces of inir's three
families shot on both. Ryzen 5000 APU (Vega), Linux 7.2, Qt 6.12, Mesa 26.2.

</details>

A single benchmark is never the whole truth, but this one is pretty loud.

## Why one shell

- General runtimes pay for every config they might meet. qs-rs only meets inir.
- Both sides bend: when changing inir makes the pair faster, inir changes too.
- Upstream parity is the floor, not the ceiling. Whatever is missing gets built, and better.

Rust owns all state and I/O off the GUI thread; Qt only draws.

## Progress

Same A/B, same machine. CPU is shown against Quickshell in that run, since it drifts by day.

```
  date        what landed                                     load   CPU vs qs      RSS
  2026-10-16  first full inir session                       632 ms        -47%   472 MB
  2026-10-17  hot reload                                    632 ms        -49%   478 MB
  2026-10-17  deferred cache writes, Mesa variants skipped  657 ms        -62%   479 MB
  2026-10-17  per-frame animations, windows kept on reload  632 ms        -62%   477 MB
```

<details>
<summary>How each area landed</summary>

- **2026-10-16, past midnight:** QML host, processes, FileView, IPC, layer shell, screens,
  clipboard, Hyprland IPC.
- **2026-10-16, early morning:** PipeWire, UPower, Bluetooth, MPRIS, notifications, system tray
  and menus, popups, idle monitor, toplevels, session lock, PAM.
- **2026-10-16, morning:** NetworkManager, Polkit, desktop entries. inir runs daily on it from here.
- **2026-10-16, night:** hot reload; notifications, tray and Polkit survive it.
- **2026-10-17:** cold start 6.9 s to 2.5 s, control panel peak 723 MB to 513 MB, animations paced
  to the screen, windows kept across a reload, `qs errors`.

</details>

## Status

inir runs on it as a full daily session, all three families (ii, Waffle, iRiS).

| area | state |
|:---|:---|
| core, windows, popups, hot reload | done |
| Io: Process, FileView, JsonAdapter, Socket, IPC | done |
| Wayland: layer shell, session lock, toplevels, idle monitor, clipboard | done |
| services: PipeWire, MPRIS, UPower, Bluetooth, NetworkManager, notifications, tray, Polkit, PAM | done |
| Hyprland | IPC done; shortcuts and focus grab inert (unused on niri) |
| `ScreencopyView` | loads; capture not ported |
| `IdleInhibitor`, `DataStream`, `SocketServer`, widget wrappers | missing; inir doesn't use them |
| I3/Sway, Greetd, X11 | out of scope |

Left that inir actually touches: `watchFiles` (its dev reload), `updatesEnabled` (the ii sidebar
stops drawing while hidden) and screen capture for `ScreencopyView`.

## Finding what breaks

```sh
qs errors            # every warning of the running shell, grouped: shell area, runtime, system
qs errors --static   # plus a compile of every QML file, all families, nothing shown
qs errors -f         # keep watching; each new kind prints as it shows up
```

## Build

```sh
cargo build --release      # Qt 6, LayerShellQt and a C++ compiler
scripts/test.sh            # every tests/qml/*.qml, isolated HOME and XDG dirs
```

## License and credits

GNU LGPL 3, same as Quickshell: [LICENSE](LICENSE), on top of the GPL in
[LICENSE-GPL](LICENSE-GPL). Semantics ported from
[Quickshell](https://git.outfoxxed.me/quickshell/quickshell) by outfoxxed and contributors; any
bug in the port is ours.

This code was written by: Hikari.
