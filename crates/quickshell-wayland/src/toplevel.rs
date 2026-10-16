//! ToplevelManager over zwlr_foreign_toplevel_management_v1. One process-wide queue/thread binds
//! the manager; handle state accumulates there and is published to every `ToplevelCore` (one per
//! QML engine) on each `done`, so QML sees atomic updates. ToplevelManager.qml keeps one Toplevel
//! (Toplevel.qml over `ToplevelBase`) per handle, inserted on the first `done` like upstream.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "ToplevelCore"]
        #[qml_singleton]
        type ToplevelCore = super::ToplevelCoreRust;

        /// Full state of a toplevel after a `done`; `parent` is 0 for none.
        #[qsignal]
        #[cxx_name = "toplevelUpdated"]
        fn toplevel_updated(
            self: Pin<&mut ToplevelCore>,
            key: u32,
            app_id: QString,
            title: QString,
            parent: u32,
            activated: bool,
            maximized: bool,
            minimized: bool,
            fullscreen: bool,
            screens: QStringList,
        );
        #[qsignal]
        #[cxx_name = "toplevelClosed"]
        fn toplevel_closed(self: Pin<&mut ToplevelCore>, key: u32);
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(u32, _qsKey, READ = key, WRITE = set_key, NOTIFY = key_changed)]
        #[qproperty(bool, maximized, READ, WRITE = request_maximized, NOTIFY)]
        #[qproperty(bool, minimized, READ, WRITE = request_minimized, NOTIFY)]
        #[qproperty(bool, fullscreen, READ, WRITE = request_fullscreen, NOTIFY)]
        type ToplevelBase = super::ToplevelBaseRust;

        fn key(self: &ToplevelBase) -> u32;
        fn set_key(self: Pin<&mut ToplevelBase>, key: u32);
        #[qsignal]
        fn key_changed(self: Pin<&mut ToplevelBase>);
        fn request_maximized(self: Pin<&mut ToplevelBase>, value: bool);
        fn request_minimized(self: Pin<&mut ToplevelBase>, value: bool);
        fn request_fullscreen(self: Pin<&mut ToplevelBase>, value: bool);

        /// Called by ToplevelManager.qml with compositor state; never sends requests.
        #[qinvokable]
        #[cxx_name = "_qsSetState"]
        fn set_state(self: Pin<&mut ToplevelBase>, maximized: bool, minimized: bool, fullscreen: bool);
        #[qinvokable]
        fn activate(self: &ToplevelBase);
        #[qinvokable]
        fn close(self: &ToplevelBase);
        /// Fullscreen on the screen with this name (empty: compositor's choice).
        #[qinvokable]
        #[cxx_name = "_qsFullscreenOn"]
        fn fullscreen_on(self: &ToplevelBase, screen: &QString);
    }

    impl cxx_qt::Initialize for ToplevelCore {}
    impl cxx_qt::Threading for ToplevelCore {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QString, QStringList};
use qs_wayland::wayland_client::globals::{registry_queue_init, GlobalListContents};
use qs_wayland::wayland_client::protocol::{wl_output::WlOutput, wl_registry, wl_seat::WlSeat};
use qs_wayland::wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle};
use qs_wayland::wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::{
    self as handle_v1, ZwlrForeignToplevelHandleV1 as Handle,
};
use qs_wayland::wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::{
    self as manager_v1, ZwlrForeignToplevelManagerV1 as Manager,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

type Qt = cxx_qt::CxxQtThread<qobject::ToplevelCore>;

#[derive(Default)]
pub struct ToplevelCoreRust;

#[derive(Default)]
pub struct ToplevelBaseRust {
    key: u32,
    maximized: bool,
    minimized: bool,
    fullscreen: bool,
}

/// What the compositor told us about one handle so far.
#[derive(Clone, Default)]
struct Info {
    app_id: String,
    title: String,
    parent: u32,
    activated: bool,
    maximized: bool,
    minimized: bool,
    fullscreen: bool,
    outputs: Vec<WlOutput>,
    ready: bool,
}

/// Shared between the dispatch thread and the Qt thread.
#[derive(Default)]
struct Shared {
    cores: Vec<Qt>,
    handles: HashMap<u32, Handle>,
    infos: HashMap<u32, Info>,
}

fn shared() -> &'static Mutex<Shared> {
    static S: OnceLock<Mutex<Shared>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn next_key() -> u32 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

struct State;

struct Globals {
    seat: Option<WlSeat>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<WlSeat, ()> for State {
    fn event(_: &mut Self, _: &WlSeat, _: <WlSeat as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<Manager, ()> for State {
    fn event(_: &mut Self, _: &Manager, event: manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let manager_v1::Event::Toplevel { toplevel } = event {
            let key = *toplevel.data::<u32>().unwrap_or(&0);
            let mut s = shared().lock().unwrap();
            s.handles.insert(key, toplevel);
            s.infos.insert(key, Info::default());
        }
    }

    event_created_child!(State, Manager, [
        manager_v1::EVT_TOPLEVEL_OPCODE => (Handle, next_key()),
    ]);
}

impl Dispatch<Handle, u32> for State {
    fn event(_: &mut Self, handle: &Handle, event: handle_v1::Event, key: &u32, _: &Connection, _: &QueueHandle<Self>) {
        let key = *key;
        let mut s = shared().lock().unwrap();
        if let handle_v1::Event::Closed = event {
            handle.destroy();
            s.handles.remove(&key);
            let was_ready = s.infos.remove(&key).is_some_and(|i| i.ready);
            // Children lose their parent, as upstream's onParentClosed does.
            let orphans: Vec<u32> = s.infos.iter_mut().filter(|(_, i)| i.parent == key).map(|(k, i)| { i.parent = 0; *k }).collect();
            for k in orphans {
                publish(&mut s, k);
            }
            if was_ready {
                s.cores.retain(|qt| qt.queue(move |this: Pin<&mut qobject::ToplevelCore>| this.toplevel_closed(key)).is_ok());
            }
            return;
        }
        let Some(info) = s.infos.get_mut(&key) else { return };
        match event {
            handle_v1::Event::Title { title } => info.title = title,
            handle_v1::Event::AppId { app_id } => info.app_id = app_id,
            handle_v1::Event::OutputEnter { output } => {
                if !info.outputs.contains(&output) {
                    info.outputs.push(output);
                }
            }
            handle_v1::Event::OutputLeave { output } => info.outputs.retain(|o| o != &output),
            handle_v1::Event::State { state } => {
                let flags: Vec<u32> = state.chunks_exact(4).map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect();
                let has = |s: handle_v1::State| flags.contains(&(s as u32));
                info.activated = has(handle_v1::State::Activated);
                info.maximized = has(handle_v1::State::Maximized);
                info.minimized = has(handle_v1::State::Minimized);
                info.fullscreen = has(handle_v1::State::Fullscreen);
            }
            handle_v1::Event::Parent { parent } => info.parent = parent.and_then(|p| p.data::<u32>().copied()).unwrap_or(0),
            handle_v1::Event::Done => {
                info.ready = true;
                publish(&mut s, key);
            }
            _ => {}
        }
    }
}

/// Queues the current state of `key` to every core, dropping cores whose object is gone.
fn publish(s: &mut Shared, key: u32) {
    let Some(info) = s.infos.get(&key).filter(|i| i.ready).cloned() else { return };
    s.cores.retain(|qt| {
        let info = info.clone();
        qt.queue(move |this: Pin<&mut qobject::ToplevelCore>| emit_update(this, key, &info)).is_ok()
    });
}

fn emit_update(this: Pin<&mut qobject::ToplevelCore>, key: u32, i: &Info) {
    let mut screens = QStringList::default();
    for o in &i.outputs {
        let name = qs_wayland::screen_name_for_output(o);
        if !name.is_empty() {
            screens.append(QString::from(&name));
        }
    }
    this.toplevel_updated(key, QString::from(&i.app_id), QString::from(&i.title), i.parent, i.activated, i.maximized, i.minimized, i.fullscreen, screens);
}

/// Binds the manager on its own queue; `None` (with a warning, once) when the compositor lacks
/// the protocol or Qt is not on Wayland. Qt thread only on first call.
fn globals() -> Option<&'static Globals> {
    static G: OnceLock<Option<Globals>> = OnceLock::new();
    G.get_or_init(|| {
        let conn = qs_wayland::connection()?;
        let (list, queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();
        if list.bind::<Manager, _, _>(&qh, 1..=3, ()).is_err() {
            eprintln!("Cannot manage toplevels as zwlr_foreign_toplevel_management_v1 is not supported by the current compositor.");
            return None;
        }
        let seat = list.bind::<WlSeat, _, _>(&qh, 1..=1, ()).ok();
        qs_wayland::spawn_dispatch("wayland-toplevel", queue, State);
        Some(Globals { seat })
    })
    .as_ref()
}

impl cxx_qt::Initialize for qobject::ToplevelCore {
    fn initialize(self: Pin<&mut Self>) {
        if globals().is_none() {
            return;
        }
        let qt = self.qt_thread();
        let mut s = shared().lock().unwrap();
        // A new engine (reload) starts from every toplevel that is already ready.
        let ready: Vec<(u32, Info)> = s.infos.iter().filter(|(_, i)| i.ready).map(|(k, i)| (*k, i.clone())).collect();
        let _ = qt.queue(move |mut this: Pin<&mut qobject::ToplevelCore>| {
            for (key, info) in &ready {
                emit_update(this.as_mut(), *key, info);
            }
        });
        s.cores.push(qt);
    }
}

fn with_handle(key: u32, f: impl FnOnce(&Handle)) {
    let s = shared().lock().unwrap();
    if let Some(h) = s.handles.get(&key) {
        f(h);
        drop(s);
        if let Some(c) = qs_wayland::connection() {
            let _ = c.flush();
        }
    }
}

impl qobject::ToplevelBase {
    fn key(&self) -> u32 {
        self.rust().key
    }

    fn set_key(mut self: Pin<&mut Self>, key: u32) {
        self.as_mut().rust_mut().key = key;
        self.key_changed();
    }

    fn request_maximized(self: Pin<&mut Self>, value: bool) {
        with_handle(self.rust().key, |h| if value { h.set_maximized() } else { h.unset_maximized() });
    }

    fn request_minimized(self: Pin<&mut Self>, value: bool) {
        with_handle(self.rust().key, |h| if value { h.set_minimized() } else { h.unset_minimized() });
    }

    fn request_fullscreen(self: Pin<&mut Self>, value: bool) {
        with_handle(self.rust().key, |h| if value { h.set_fullscreen(None) } else { h.unset_fullscreen() });
    }

    fn set_state(mut self: Pin<&mut Self>, maximized: bool, minimized: bool, fullscreen: bool) {
        if self.maximized != maximized {
            self.as_mut().rust_mut().maximized = maximized;
            self.as_mut().maximized_changed();
        }
        if self.minimized != minimized {
            self.as_mut().rust_mut().minimized = minimized;
            self.as_mut().minimized_changed();
        }
        if self.fullscreen != fullscreen {
            self.as_mut().rust_mut().fullscreen = fullscreen;
            self.as_mut().fullscreen_changed();
        }
    }

    fn activate(&self) {
        let Some(seat) = globals().and_then(|g| g.seat.as_ref()) else { return };
        with_handle(self.rust().key, |h| h.activate(seat));
    }

    fn close(&self) {
        with_handle(self.rust().key, |h| h.close());
    }

    fn fullscreen_on(&self, screen: &QString) {
        let output = qs_wayland::output_for_screen_name(&screen.to_string());
        with_handle(self.rust().key, |h| h.set_fullscreen(output.as_ref()));
    }
}
