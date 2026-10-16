//! The org.freedesktop.Notifications server. One zbus connection serves the interface on its
//! own threads and claims the name (retrying when the current owner leaves, like upstream's
//! service watcher). Notify allocates ids here so it can answer synchronously; the QML server
//! decides tracking and reports every notification that leaves through `closed`.

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

        /// Sets the advertised capabilities (space separated) and starts serving on first call.
        #[qinvokable]
        fn start(self: &NotificationServerCore, capabilities: &QString);
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
use qs_dbus::{json_string, props_json, Worker};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use zbus::blocking::{Connection, MessageIterator};
use zbus::fdo::RequestNameFlags;
use zbus::message::Type;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;
use zbus::MatchRule;

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

type Qt = CxxQtThread<qobject::NotificationServerCore>;

struct State {
    next_id: u32,
    /// Ids the server still holds, with the file of their current pixmap.
    live: HashMap<u32, Option<PathBuf>>,
    capabilities: Vec<String>,
    image_seq: u64,
    image_dir: PathBuf,
}

#[derive(Default)]
pub struct NotificationServerCoreRust {
    state: Arc<Mutex<Option<State>>>,
    worker: Arc<OnceLock<Worker>>,
}

struct Server {
    state: Arc<Mutex<Option<State>>>,
    qt: Qt,
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
        if let Some(Some(old)) = st.live.insert(id, None) {
            let _ = std::fs::remove_file(old);
        }
        let mut image = String::new();
        if let Some(p) = pixmap {
            st.image_seq += 1;
            let file = st.image_dir.join(format!("{id}-{}.bmp", st.image_seq));
            match std::fs::create_dir_all(&st.image_dir).and_then(|_| std::fs::write(&file, p.to_bmp())) {
                Ok(()) => {
                    image = format!("file://{}", file.display());
                    st.live.insert(id, Some(file));
                }
                Err(e) => eprintln!("Notifications: cannot write image: {e}"),
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
        let _ = self.qt.queue(move |this| this.notified(id, QString::from(&json)));
        id
    }

    fn close_notification(&self, id: u32) {
        let live = self.state.lock().unwrap().as_ref().is_some_and(|s| s.live.contains_key(&id));
        if live {
            let _ = self.qt.queue(move |this| this.close_requested(id));
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

/// Claims the name now and again whenever its owner goes away.
fn claim_name(conn: &Connection) -> zbus::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg(0, NAME)?
        .build();
    let changes = MessageIterator::for_match_rule(rule, conn, None)?;
    let try_claim = || match conn.request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into()) {
        Ok(_) => eprintln!("Notifications: registered {NAME}"),
        Err(zbus::Error::NameTaken) => eprintln!(
            "Notifications: {NAME} is owned by another server; registration will be retried when it goes away"
        ),
        Err(e) => eprintln!("Notifications: cannot register {NAME}: {e}"),
    };
    try_claim();
    for msg in changes {
        let msg = msg?;
        if let Ok((_, _, new)) = msg.body().deserialize::<(String, String, String)>() {
            if new.is_empty() {
                try_claim();
            }
        }
    }
    Ok(())
}

impl qobject::NotificationServerCore {
    fn start(&self, capabilities: &QString) {
        let caps: Vec<String> = capabilities.to_string().split_whitespace().map(String::from).collect();
        let mut guard = self.state.lock().unwrap();
        if let Some(st) = guard.as_mut() {
            st.capabilities = caps;
            return;
        }
        *guard = Some(State { next_id: 1, live: HashMap::new(), capabilities: caps, image_seq: 0, image_dir: image_dir("notifications") });
        drop(guard);

        let (state, worker, qt) = (self.state.clone(), self.worker.clone(), self.qt_thread());
        std::thread::Builder::new()
            .name("notifications".into())
            .spawn(move || {
                let conn = zbus::blocking::connection::Builder::session()
                    .and_then(|b| b.serve_at(PATH, Server { state, qt }))
                    .and_then(|b| b.build());
                let conn = match conn {
                    Ok(c) => c,
                    Err(e) => return eprintln!("Notifications: cannot serve on the session bus: {e}"),
                };
                worker.get_or_init(|| Worker::new(conn.clone(), "notifications-signals"));
                if let Err(e) = claim_name(&conn) {
                    eprintln!("Notifications: {e}");
                }
            })
            .expect("spawn notifications thread");
    }

    fn emit(&self, what: &'static str, f: impl FnOnce(&SignalEmitter<'static>) -> zbus::Result<()> + Send + 'static) {
        if let Some(worker) = self.worker.get() {
            worker.run(format!("Notifications: {what}"), move |conn| {
                let iface = conn.object_server().interface::<_, Server>(PATH)?;
                f(iface.signal_emitter())
            });
        }
    }

    fn closed(&self, id: u32, reason: u32) {
        if let Some(st) = self.state.lock().unwrap().as_mut() {
            if let Some(Some(file)) = st.live.remove(&id) {
                let _ = std::fs::remove_file(file);
            }
        }
        self.emit("NotificationClosed", move |e| zbus::block_on(Server::notification_closed(e, id, reason)));
    }

    fn invoke_action(&self, id: u32, action: &QString) {
        let action = action.to_string();
        self.emit("ActionInvoked", move |e| zbus::block_on(Server::action_invoked(e, id, &action)));
    }

    fn reply(&self, id: u32, text: &QString) {
        let text = text.to_string();
        self.emit("NotificationReplied", move |e| zbus::block_on(Server::notification_replied(e, id, &text)));
    }
}
