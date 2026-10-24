//! The org.freedesktop.Notifications server. One zbus connection serves the interface on its
//! own threads and claims the name (retrying when the current owner leaves, like upstream's
//! service watcher). Notify allocates ids here so it can answer synchronously; the QML server
//! decides tracking and reports every notification that leaves through `closed`.
//! The service is one per process, like upstream's NotificationServer::instance(): each shell
//! generation attaches to it, so a reload neither re-serves the name nor loses notifications.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "NotificationServerCore"]
        #[qml_singleton]
        type NotificationServerCore = super::NotificationServerCoreRust;

        /// Sets the advertised capabilities (space separated), makes this generation the one that
        /// gets notifications and starts serving on first call. The first call of a generation
        /// re-emits the live notifications of the last one (`keep`) or closes them as expired.
        #[qinvokable]
        fn start(self: &NotificationServerCore, capabilities: &QString, keep: bool);
        /// A notification left the server: emits NotificationClosed and frees its id.
        #[qinvokable]
        fn closed(self: &NotificationServerCore, id: u32, reason: u32);
        #[qinvokable]
        #[cxx_name = "invokeAction"]
        fn invoke_action(self: &NotificationServerCore, id: u32, action: &QString);
        #[qinvokable]
        fn reply(self: &NotificationServerCore, id: u32, text: &QString);

        /// A new or replacing notification, as JSON (appName, appIcon, summary, body, actions,
        /// hints, expireTimeout, image).
        #[qsignal]
        fn notified(self: Pin<&mut NotificationServerCore>, id: u32, json: QString);
        /// CloseNotification from a client.
        #[qsignal]
        #[cxx_name = "closeRequested"]
        fn close_requested(self: Pin<&mut NotificationServerCore>, id: u32);
    }

    impl cxx_qt::Threading for NotificationServerCore {}

    extern "RustQt" {
        /// Base of Notification.qml: `transient` is a reserved word in QML declarations.
        #[qobject]
        #[qml_element]
        #[qproperty(bool, transient)]
        type NotificationBase = super::NotificationBaseRust;
    }
}

#[derive(Default)]
pub struct NotificationBaseRust {
    transient: bool,
}

use cxx_qt::{CxxQtThread, Threading};
use cxx_qt_lib::QString;
use qs_dbus::pixmap::{image_dir, Pixmap};
use qs_dbus::{json_string, props_json, Bus, Worker};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

type Qt = CxxQtThread<qobject::NotificationServerCore>;

struct State {
    next_id: u32,
    /// Ids the server still holds, with the file of their current pixmap.
    live: HashMap<u32, Option<PathBuf>>,
    /// The last JSON sent for each live id, re-emitted to the next generation on reload.
    sent: HashMap<u32, String>,
    capabilities: Vec<String>,
    image_seq: u64,
    image_dir: PathBuf,
}

/// The generation notifications go to: its core's token and thread. None while none is attached.
type Target = Arc<Mutex<Option<(u64, Qt)>>>;

struct Shared {
    state: Arc<Mutex<Option<State>>>,
    target: Target,
    /// Signals go out in order, after the interface is served.
    worker: Worker,
}

fn shared() -> &'static Shared {
    static SHARED: std::sync::OnceLock<Shared> = std::sync::OnceLock::new();
    SHARED.get_or_init(|| Shared { state: Arc::default(), target: Arc::default(), worker: Worker::new(Bus::Session, "Notifications") })
}

fn queue(target: &Target, f: impl FnOnce(std::pin::Pin<&mut qobject::NotificationServerCore>) + Send + 'static) {
    if let Some((_, qt)) = target.lock().unwrap().as_ref() {
        let _ = qt.queue(f);
    }
}

pub struct NotificationServerCoreRust {
    token: u64,
    attached: std::cell::Cell<bool>,
}

impl Default for NotificationServerCoreRust {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self { token: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed), attached: Default::default() }
    }
}

impl Drop for NotificationServerCoreRust {
    fn drop(&mut self) {
        let mut target = shared().target.lock().unwrap();
        if target.as_ref().is_some_and(|(t, _)| *t == self.token) {
            *target = None;
        }
    }
}

struct Server {
    state: Arc<Mutex<Option<State>>>,
    target: Target,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        mut hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let pixmap = ["image-data", "image_data", "icon_data"].iter().find_map(|k| hints.get(*k)).and_then(|v| Pixmap::from_value(v));
        // Don't carry large byte arrays into QML.
        for k in ["image-data", "image_data", "icon_data"] {
            hints.remove(k);
        }

        let mut guard = self.state.lock().unwrap();
        let st = guard.as_mut().expect("server started");
        let id = if replaces_id != 0 && st.live.contains_key(&replaces_id) {
            replaces_id
        } else {
            let id = st.next_id;
            st.next_id = st.next_id.wrapping_add(1).max(1);
            id
        };
        // A replaced image is removed only after QML moved to the new one.
        let stale = st.live.insert(id, None).flatten();
        let mut image = String::new();
        if let Some(p) = pixmap {
            st.image_seq += 1;
            let file = st.image_dir.join(format!("{id}-{}.bmp", st.image_seq));
            match std::fs::create_dir_all(&st.image_dir).and_then(|_| std::fs::write(&file, p.to_bmp())) {
                Ok(()) => {
                    image = format!("file://{}", file.display());
                    st.live.insert(id, Some(file));
                }
                Err(e) => quickshell::qs_log!(warn, "quickshell.service.notifications", "cannot write image: {e}"),
            }
        }
        drop(guard);

        let mut json = String::from("{\"appName\":");
        json_string(&app_name, &mut json);
        json.push_str(",\"appIcon\":");
        json_string(&app_icon, &mut json);
        json.push_str(",\"summary\":");
        json_string(&summary, &mut json);
        json.push_str(",\"body\":");
        json_string(&body, &mut json);
        json.push_str(",\"actions\":[");
        for (i, a) in actions.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            json_string(a, &mut json);
        }
        json.push_str("],\"hints\":");
        json.push_str(&props_json(hints.iter().map(|(k, v)| (k, &**v))));
        json.push_str(&format!(",\"expireTimeout\":{expire_timeout},\"image\":"));
        json_string(&image, &mut json);
        json.push('}');
        if let Some(st) = self.state.lock().unwrap().as_mut()
            && st.live.contains_key(&id) {
                st.sent.insert(id, json.clone());
            }
        queue(&self.target, move |this| {
            this.notified(id, QString::from(&json));
            if let Some(f) = stale {
                let _ = std::fs::remove_file(f);
            }
        });
        id
    }

    fn close_notification(&self, id: u32) {
        let live = self.state.lock().unwrap().as_ref().is_some_and(|s| s.live.contains_key(&id));
        if live {
            queue(&self.target, move |this| this.close_requested(id));
        }
    }

    fn get_capabilities(&self) -> Vec<String> {
        self.state.lock().unwrap().as_ref().map(|s| s.capabilities.clone()).unwrap_or_default()
    }

    #[zbus(out_args("name", "vendor", "version", "spec_version"))]
    fn get_server_information(&self) -> (String, String, String, String) {
        ("quickshell".into(), "quickshell".into(), env!("CARGO_PKG_VERSION").into(), "1.2".into())
    }

    #[zbus(signal)]
    async fn notification_closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn notification_replied(emitter: &SignalEmitter<'_>, id: u32, text: &str) -> zbus::Result<()>;
}

impl qobject::NotificationServerCore {
    fn start(&self, capabilities: &QString, keep: bool) {
        let caps: Vec<String> = capabilities.to_string().split_whitespace().map(String::from).collect();
        let sh = shared();
        let first = !self.attached.replace(true);
        if first {
            *sh.target.lock().unwrap() = Some((self.token, self.qt_thread()));
        }
        let mut guard = sh.state.lock().unwrap();
        if let Some(st) = guard.as_mut() {
            st.capabilities = caps;
            if !first {
                return;
            }
            // A reload: the last generation's notifications go to this one (upstream switchGeneration).
            let mut carried: Vec<(u32, String)> = st.sent.iter().map(|(id, j)| (*id, j.clone())).collect();
            carried.sort_by_key(|(id, _)| *id);
            drop(guard);
            if keep {
                let _ = self.qt_thread().queue(move |mut this| {
                    for (id, json) in carried {
                        let json = format!("{},\"lastGeneration\":true}}", &json[..json.len() - 1]);
                        this.as_mut().notified(id, QString::from(&json));
                    }
                });
            } else {
                for (id, _) in carried {
                    self.closed(id, 1);
                }
            }
            return;
        }
        *guard = Some(State { next_id: 1, live: HashMap::new(), sent: HashMap::new(), capabilities: caps, image_seq: 0, image_dir: image_dir("notifications") });
        drop(guard);

        let (state, target) = (sh.state.clone(), sh.target.clone());
        sh.worker.run("Notifications: serving".into(), move |conn| async move {
            conn.object_server().at(PATH, Server { state, target }).await?;
            qs_dbus::rt::spawn(async move {
                if let Err(e) = qs_dbus::claim_name(&conn, NAME, "Notifications").await {
                    quickshell::qs_log!(warn, "quickshell.service.notifications", "{e}");
                }
            });
            Ok(())
        });
    }

    fn emit<F>(&self, what: &'static str, f: impl FnOnce(SignalEmitter<'static>) -> F + Send + 'static)
    where
        F: std::future::Future<Output = zbus::Result<()>> + Send + 'static,
    {
        shared().worker.run(format!("Notifications: {what}"), move |conn| async move {
            let iface = conn.object_server().interface::<_, Server>(PATH).await?;
            f(iface.signal_emitter().clone()).await
        });
    }
    fn closed(&self, id: u32, reason: u32) {
        if let Some(st) = shared().state.lock().unwrap().as_mut() {
            st.sent.remove(&id);
            if let Some(Some(file)) = st.live.remove(&id) {
                let _ = std::fs::remove_file(file);
            }
        }
        self.emit("NotificationClosed", move |e| async move { Server::notification_closed(&e, id, reason).await });
    }

    fn invoke_action(&self, id: u32, action: &QString) {
        let action = action.to_string();
        self.emit("ActionInvoked", move |e| async move { Server::action_invoked(&e, id, &action).await });
    }

    fn reply(&self, id: u32, text: &QString) {
        let text = text.to_string();
        self.emit("NotificationReplied", move |e| async move { Server::notification_replied(&e, id, &text).await });
    }
}
