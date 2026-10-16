//! IdleMonitor over ext-idle-notify-v1. One shared queue/thread holds the notifier and a seat;
//! each monitor owns one notification, recreated whenever enabled/timeout/respectInhibitors
//! change. Events carry the monitor's generation so a replaced notification can't flip
//! `isIdle` late.

#[cxx_qt::bridge]
pub mod qobject {
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, enabled)]
        #[qproperty(f64, timeout)]
        #[qproperty(bool, respect_inhibitors, cxx_name = "respectInhibitors")]
        #[qproperty(bool, is_idle, cxx_name = "isIdle", READ, NOTIFY)]
        type IdleMonitor = super::IdleMonitorRust;
    }

    impl cxx_qt::Initialize for IdleMonitor {}
    impl cxx_qt::Threading for IdleMonitor {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use qs_wayland::wayland_client::globals::{registry_queue_init, GlobalListContents};
use qs_wayland::wayland_client::protocol::{wl_registry, wl_seat::WlSeat};
use qs_wayland::wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use qs_wayland::wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{self, ExtIdleNotificationV1};
use qs_wayland::wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;
use std::sync::OnceLock;

type Qt = cxx_qt::CxxQtThread<qobject::IdleMonitor>;

pub struct IdleMonitorRust {
    enabled: bool,
    timeout: f64,
    respect_inhibitors: bool,
    is_idle: bool,
    notification: Option<ExtIdleNotificationV1>,
    generation: u64,
    update_queued: bool,
}

impl Default for IdleMonitorRust {
    fn default() -> Self {
        Self { enabled: true, timeout: 0.0, respect_inhibitors: true, is_idle: false, notification: None, generation: 0, update_queued: false }
    }
}

struct State;

struct Globals {
    notifier: ExtIdleNotifierV1,
    seat: WlSeat,
    qh: QueueHandle<State>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<ExtIdleNotifierV1, ()> for State {
    fn event(_: &mut Self, _: &ExtIdleNotifierV1, _: <ExtIdleNotifierV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<WlSeat, ()> for State {
    fn event(_: &mut Self, _: &WlSeat, _: <WlSeat as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<ExtIdleNotificationV1, (Qt, u64)> for State {
    fn event(_: &mut Self, _: &ExtIdleNotificationV1, event: ext_idle_notification_v1::Event, data: &(Qt, u64), _: &Connection, _: &QueueHandle<Self>) {
        let idle = matches!(event, ext_idle_notification_v1::Event::Idled);
        let generation = data.1;
        let _ = data.0.queue(move |this: Pin<&mut qobject::IdleMonitor>| {
            if this.generation == generation {
                this.set_idle(idle);
            }
        });
    }
}

/// The notifier and a seat on a dedicated queue; `None` (with a warning, once) when the
/// compositor lacks ext-idle-notify-v1 or Qt is not on Wayland. Qt thread only on first call.
fn globals() -> Option<&'static Globals> {
    static G: OnceLock<Option<Globals>> = OnceLock::new();
    G.get_or_init(|| {
        let conn = qs_wayland::connection()?;
        let (list, queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();
        let notifier = match list.bind::<ExtIdleNotifierV1, _, _>(&qh, 1..=2, ()) {
            Ok(n) => n,
            Err(_) => {
                quickshell::qs_log!(warn, "quickshell.wayland.idle_notify", "Cannot create idle monitor as ext-idle-notify-v1 is not supported by the current compositor.");
                return None;
            }
        };
        let seat = list.bind::<WlSeat, _, _>(&qh, 1..=1, ()).ok()?;
        qs_wayland::spawn_dispatch("wayland-idle", queue, State);
        Some(Globals { notifier, seat, qh })
    })
    .as_ref()
}

impl cxx_qt::Initialize for qobject::IdleMonitor {
    fn initialize(mut self: Pin<&mut Self>) {
        // Property changes (including the initial QML assignments) coalesce into one update.
        self.as_mut().on_enabled_changed(|this| this.queue_update()).release();
        self.as_mut().on_timeout_changed(|this| this.queue_update()).release();
        self.as_mut().on_respect_inhibitors_changed(|this| this.queue_update()).release();
        self.queue_update();
    }
}

impl qobject::IdleMonitor {
    fn set_idle(mut self: Pin<&mut Self>, idle: bool) {
        if self.is_idle == idle {
            return;
        }
        self.as_mut().rust_mut().is_idle = idle;
        self.is_idle_changed();
    }

    fn queue_update(mut self: Pin<&mut Self>) {
        if self.update_queued {
            return;
        }
        self.as_mut().rust_mut().update_queued = true;
        let _ = self.qt_thread().queue(|this| this.update());
    }

    fn update(mut self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        let mut rust = self.as_mut().rust_mut();
        rust.update_queued = false;
        rust.generation += 1;
        if let Some(n) = rust.notification.take() {
            n.destroy();
        }
        let (enabled, timeout, mut respect, generation) = (rust.enabled, rust.timeout, rust.respect_inhibitors, rust.generation);
        if enabled {
            if let Some(g) = globals() {
                let ms = (timeout * 1000.0).max(0.0) as u32;
                if !respect && g.notifier.version() < 2 {
                    quickshell::qs_log!(warn, "quickshell.wayland.idle_notify", "Cannot ignore inhibitors for new idle notifier: Compositor does not support protocol version 2.");
                    respect = true;
                }
                let n = if respect {
                    g.notifier.get_idle_notification(ms, &g.seat, &g.qh, (qt, generation))
                } else {
                    g.notifier.get_input_idle_notification(ms, &g.seat, &g.qh, (qt, generation))
                };
                self.as_mut().rust_mut().notification = Some(n);
                if let Some(c) = qs_wayland::connection() {
                    let _ = c.flush();
                }
            }
        }
        self.set_idle(false);
    }
}

impl Drop for IdleMonitorRust {
    fn drop(&mut self) {
        if let Some(n) = self.notification.take() {
            n.destroy();
        }
    }
}
