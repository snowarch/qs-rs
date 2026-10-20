# qs-rs

Quickshell, rewritten in Rust, for exactly one shell: [iNiR](https://github.com/snowarch/inir).
Same QML API as Quickshell 0.3.2, same CLI and IPC, and the process is still called `qs`, so
inir doesn't notice the swap. It just gets lighter.

> [!WARNING]
> **Written by an AI, on autopilot.** Every line here was written autonomously by Hikari, a
> Claude Code agent (Claude Opus 5.5, `claude-opus-5-5`, high reasoning effort; each session
> hands over to a fresh one at 300-350k tokens of context). The maintainer steers by what shows
> up on screen and doesn't read the code. That makes qs-rs an experiment too: how far an AI can
> take a project like this almost on its own.
>
> Through 2026-10-19: 32 sessions over four days, 3,564 model calls, 3.1M tokens written and 715M
> read (almost all of it cache: every call re-reads the session so far).
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
- **2026-10-18:** `watchFiles` (save to reload in 0.16 s, one reload per burst of saves),
  `updatesEnabled`, and WebEngine: inir's sidebar web apps run here, which stock Quickshell can't do.
  Screen capture too: inir's anti-flashbang now reads the screen in 8 ms instead of 328 ms of
  `grim | magick`, so it dims before the flash instead of after.
- **2026-10-19:** live window capture on niri (dmabuf, zero-copy), the QML stack on fatal signals,
  object lists that outlive their owners as JS arrays (a game closing dozens of windows used to
  take the shell down with it).
- **2026-10-20:** reloads in a row no longer crash (no JS runs while the old generation comes
  apart, like upstream), SIGTERM quits cleanly, tray entries an app takes back go inert instead of
  dying under a click, and every new memory peak is logged with the windows that were open. Two
  things inir changes live now land live: a reserved screen edge (a layer surface that never
  redraws) and a new video wallpaper (`Process` starts on the spot again, like upstream).

</details>

## Status

inir runs on it as a full daily session, all three families (ii, Waffle, iRiS).

| area | state |
|:---|:---|
| core, windows, popups, hot reload, `watchFiles`, `updatesEnabled` | done |
| WebEngine (`hasVersion(0, 3, ["webengine"])`) | done; not something Quickshell has |
| Io: Process, FileView, JsonAdapter, Socket, IPC | done |
| Wayland: layer shell, session lock, toplevels, idle monitor, clipboard | done |
| services: PipeWire, MPRIS, UPower, Bluetooth, NetworkManager, notifications, tray, Polkit, PAM | done |
| Hyprland | IPC done; shortcuts and focus grab inert (unused on niri) |
| `ScreencopyView` | screens done (wlr-screencopy, shared memory); live windows on niri, zero-copy (see below) |
| `IdleInhibitor`, `DataStream`, `SocketServer`, widget wrappers | missing; inir doesn't use them |
| I3/Sway, Greetd, X11 | out of scope |

Left that inir actually touches: its region selector and recorder still shell out to `grim`,
and screen captures still go through the CPU (window captures already don't).

Window previews are live, not screenshots: `ScreencopyView { niriWindowId: id }` (or a Toplevel)
asks niri's ScreenCast for that one window and draws the PipeWire dmabuf frames as GPU textures,
no copy on the way. niri keeps a recording whose client died, so qs-rs stops its own on exit and
sweeps what a killed shell left behind.

When it does crash, it says where: a fatal signal prints the native backtrace and then the QML
stack of every engine, `file.qml:line` included, even halfway through a reload.

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
