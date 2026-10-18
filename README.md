# qs-rs

Quickshell, rewritten in Rust, for exactly one shell: [iNiR](https://github.com/snowarch/inir).

Same QML API as Quickshell 0.3.2, same CLI, IPC socket and log files, and its process is named
`qs`, so inir's launcher, niri binds and `qs ipc call` don't notice the swap. They just get
lighter.

## Scope

What it is: Quickshell 0.3.2's QML API, CLI, IPC socket and log layout, for inir on niri
(Wayland). Whatever inir names has to load and behave like upstream, and every type gets a test
before the next one starts.

What it is not: a general Quickshell replacement. No X11, no i3/Sway IPC, no Greetd, no
ext-workspace window manager module. The Hyprland module is there because inir's code names it,
but on niri its live parts stay quiet.

Why only one shell: general runtimes pay for every config they might meet. qs-rs only meets inir,
so both sides bend toward each other: when a change in inir makes the pair faster, inir changes
too. Upstream parity is the floor, not the ceiling.

Rust owns all state and I/O (processes, files, D-Bus, PipeWire, Wayland protocols) off the GUI
thread; Qt only draws. One D-Bus thread per bus, one poll thread for every child process, one
inotify thread for every file watcher, one GL context for every window.

## Numbers

inir on both runtimes, same config and env, each in its own headless KWin, warm QML cache,
sampled 20 s after start and after a minute idle (mean of 3 runs each, 2026-10-17):

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

Quickshell never keeps a QML cache for a shell, so every start is its cold start; qs-rs compiles
again only when an update changes a type the shell uses. Opening inir's control panel peaks at
806 MB on Quickshell and 513 MB on qs-rs (Mesa's optimized shader variants, compiled at every
start, are skipped). Opening any panel spends at most 3 ms preparing its first frame (Quickshell:
8 ms).

Same pixels on screen: 191 surfaces of inir's three families shot on both. Ryzen 5000 APU (Vega),
Linux 7.2, Qt 6.12, Mesa 26.2. A single benchmark is never the whole truth, but this one is pretty
loud.

## Progress

Every row is the same A/B on the same machine. Absolute CPU drifts from day to day, so it is
shown against Quickshell in that same run.

| date | what changed | load | CPU to start vs Quickshell | threads | RSS |
|---|---|---:|---:|---:|---:|
| 2026-10-16 | first full inir session | 632 ms | -47% | 40 | 472 MB |
| 2026-10-17 | after hot reload landed | 632 ms | -49% | 38 | 478 MB |
| 2026-10-17 | QML cache writes deferred, Mesa shader variants skipped | 657 ms | -62% | 40 | 479 MB |
| 2026-10-17 | animations step per frame, windows kept across reloads | 632 ms | -62% | 38 | 477 MB |

How the areas landed:

- **2026-10-16, past midnight:** the QML host, import scanning, processes, FileView and
  JsonAdapter, IPC, layer shell panels and floating windows, screens, clipboard, Hyprland IPC.
- **2026-10-16, early morning:** PipeWire, UPower and power profiles, Bluetooth, MPRIS, system
  clock, notifications server, system tray, popups, idle monitor, toplevels, session lock, PAM,
  tray menus.
- **2026-10-16, morning:** background effect, IPC wait/listen, desktop entries, NetworkManager,
  Polkit agent. inir runs as a daily session from here on.
- **2026-10-16, night:** hot reload. Notifications, tray and Polkit keep one registration across
  reloads, so a reload never drops a notification or the agent.
- **2026-10-17:** cold start 6.9 s to 2.5 s (Qt synced every cache file to disk; now one sync),
  control panel peak 723 MB to 513 MB, animations paced to the screen, shown windows kept across a
  reload (one layer surface recreated instead of eleven), and a reload into another family drops
  the old windows as soon as the new ones show.

## Status

inir runs on it as a full daily session, all three families (ii, Waffle, iRiS).

| area | state |
|---|---|
| core: ShellRoot, Scope, Variants, LazyLoader, singletons, hot reload | done; a reload keeps the shown windows |
| windows: PanelWindow, FloatingWindow, PopupWindow, masks | done; `updatesEnabled` and `surfaceFormat` accepted but inert |
| Io: Process, FileView, JsonAdapter, Socket, IpcHandler | done; `DataStream`, `SocketServer` missing |
| Wayland: layer shell, session lock, toplevels, idle monitor, background effect, clipboard | done |
| Wayland: `ScreencopyView` | the type loads, nothing is captured yet |
| Wayland: `IdleInhibitor`, `ShortcutsInhibitor` | missing |
| services: PipeWire, MPRIS, UPower, Bluetooth, NetworkManager, notifications, tray and menus, Polkit, PAM | done |
| Hyprland: IPC, workspaces, toplevels | done; `GlobalShortcut` and `HyprlandFocusGrab` inert |
| Widgets: IconImage, ClippingRectangle | done; the wrapper types missing |
| I3/Sway, WindowManager, Greetd, X11 | out of scope |

What is left that inir actually touches:

- `Quickshell.watchFiles`: inir's developer hot reload sets it; nothing reads it yet.
- `updatesEnabled`: the ii sidebar uses it to stop drawing while hidden; today it keeps drawing.
- `ScreencopyView` pixels for screens. Window previews can't be captured on niri even by
  Quickshell (niri has no window capture protocol), so inir's overview uses screenshots there.

The rest of the missing list, inir never asks for. Shells other than inir may trip over types
nobody asked for yet.

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
