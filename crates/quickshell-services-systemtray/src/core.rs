//! StatusNotifierItem tray. One connection hosts org.kde.StatusNotifierWatcher (claimed again
//! whenever another watcher leaves, like upstream); another one is the host: it registers with
//! whichever watcher owns the name, reads every item with GetAll and re-reads it on the item's
//! New* signals. Items are keyed by the watcher's "service/path" string.
//! Watcher and host run once per process, like upstream's StatusNotifierWatcher::instance(): each
//! shell generation attaches to the host and gets the current items, so a reload neither
//! re-claims the watcher name nor loses the tray.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "SystemTrayCore"]
        #[qml_singleton]
        type SystemTrayCore = super::SystemTrayCoreRust;

        #[qinvokable]
        fn activate(self: &SystemTrayCore, key: &QString);
        #[qinvokable]
        #[cxx_name = "secondaryActivate"]
        fn secondary_activate(self: &SystemTrayCore, key: &QString);
        #[qinvokable]
        fn scroll(self: &SystemTrayCore, key: &QString, delta: i32, horizontal: bool);

        /// An item's properties as JSON; the first one makes the item ready.
        #[qsignal]
        #[cxx_name = "itemChanged"]
        fn item_changed(self: Pin<&mut SystemTrayCore>, key: QString, json: QString);
        #[qsignal]
        #[cxx_name = "itemRemoved"]
        fn item_removed(self: Pin<&mut SystemTrayCore>, key: QString);
    }

    impl cxx_qt::Initialize for SystemTrayCore {}
    impl cxx_qt::Threading for SystemTrayCore {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtThread, Threading};
use cxx_qt_lib::QString;
use qs_dbus::pixmap::{image_dir, Pixmap};
use qs_dbus::rt::{next, spawn};
use qs_dbus::{json_string, Bus, Worker};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use zbus::message::{Header, Type};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, MatchRule, Proxy};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";

type Qt = CxxQtThread<qobject::SystemTrayCore>;

pub struct SystemTrayCoreRust {
    worker: Worker,
    token: u64,
}

impl Default for SystemTrayCoreRust {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self { worker: Worker::new(Bus::Session, "SystemTray"), token: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) }
    }
}

impl Drop for SystemTrayCoreRust {
    fn drop(&mut self) {
        let mut target = tray().target.lock().unwrap();
        if target.as_ref().is_some_and(|(t, _)| *t == self.token) {
            *target = None;
        }
    }
}

/// The generation tray items go to: its core's token and thread. None while none is attached.
type Target = Arc<Mutex<Option<(u64, Qt)>>>;

struct Tray {
    target: Target,
    items: Arc<Mutex<HashMap<String, Item>>>,
    started: std::sync::Once,
}

fn tray() -> &'static Tray {
    static TRAY: std::sync::OnceLock<Tray> = std::sync::OnceLock::new();
    TRAY.get_or_init(|| Tray { target: Arc::default(), items: Arc::default(), started: std::sync::Once::new() })
}

/// Queues `f` on the attached generation; false when there is none.
fn queue(target: &Target, f: impl FnOnce(Pin<&mut qobject::SystemTrayCore>) + Send + 'static) -> bool {
    match target.lock().unwrap().as_ref() {
        Some((_, qt)) => qt.queue(f).is_ok(),
        None => false,
    }
}

// ---- watcher ----

#[derive(Default)]
struct Watcher {
    items: Arc<Mutex<Vec<String>>>,
    hosts: Arc<Mutex<Vec<String>>>,
}

/// A registration argument as "service/path": a bare path belongs to the caller, a bare
/// service uses the default path.
fn qualify(item: &str, sender: Option<&str>) -> String {
    let mut q = if item.starts_with('/') { format!("{}{item}", sender.unwrap_or_default()) } else { item.to_string() };
    if !q.contains('/') {
        q.push_str("/StatusNotifierItem");
    }
    q
}

async fn has_owner(conn: &zbus::Connection, name: &str) -> bool {
    let Ok(dbus) = zbus::fdo::DBusProxy::new(conn).await else { return false };
    let Ok(name) = zbus::names::BusName::try_from(name) else { return false };
    dbus.get_name_owner(name).await.is_ok()
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let q = qualify(service, hdr.sender().map(|s| s.as_str()));
        let svc = q.split('/').next().unwrap_or_default().to_string();
        if self.items.lock().unwrap().contains(&q) {
            return;
        }
        if !has_owner(conn, &svc).await {
            return quickshell::qs_log!(warn, "quickshell.service.sni", "ignoring invalid item registration of {q}");
        }
        self.items.lock().unwrap().push(q.clone());
        let _ = Self::status_notifier_item_registered(&emitter, &q).await;
    }

    async fn register_status_notifier_host(
        &self,
        service: &str,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        if self.hosts.lock().unwrap().iter().any(|h| h == service) {
            return;
        }
        if !has_owner(conn, service).await {
            return quickshell::qs_log!(warn, "quickshell.service.sni", "ignoring invalid host registration of {service}");
        }
        self.hosts.lock().unwrap().push(service.to_string());
        let _ = Self::status_notifier_host_registered(&emitter).await;
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap().clone()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_unregistered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

async fn run_watcher() -> zbus::Result<()> {
    let watcher = Watcher::default();
    let (items, hosts) = (watcher.items.clone(), watcher.hosts.clone());
    let conn = Bus::Session.connect().await?;
    conn.object_server().at(WATCHER_PATH, watcher).await?;
    let mut changes = qs_dbus::name_owner_changes(&conn, "").await?;
    {
        let conn = conn.clone();
        spawn(async move {
            if let Err(e) = qs_dbus::claim_name(&conn, WATCHER, "SystemTray").await {
                quickshell::qs_log!(warn, "quickshell.service.sni", "{e}");
            }
        });
    }
    // Drop items and hosts whose service left the bus.
    while let Some(msg) = next(&mut changes).await {
        let Some((name, _, new)) = qs_dbus::owner_change(&msg?) else { continue };
        if !new.is_empty() {
            continue;
        }
        let prefix = format!("{name}/");
        let gone: Vec<String> = {
            let mut items = items.lock().unwrap();
            let gone = items.iter().filter(|i| i.starts_with(&prefix)).cloned().collect();
            items.retain(|i| !i.starts_with(&prefix));
            gone
        };
        let host_gone = {
            let mut hosts = hosts.lock().unwrap();
            let n = hosts.len();
            hosts.retain(|h| *h != name);
            hosts.len() != n
        };
        if gone.is_empty() && !host_gone {
            continue;
        }
        let iface = conn.object_server().interface::<_, Watcher>(WATCHER_PATH).await?;
        let emitter = iface.signal_emitter();
        for item in gone {
            let _ = Watcher::status_notifier_item_unregistered(emitter, &item).await;
        }
        if host_gone {
            let _ = Watcher::status_notifier_host_unregistered(emitter).await;
        }
    }
    Ok(())
}

// ---- host ----

struct Item {
    service: String,
    path: String,
    /// Unique name the item's signals come from.
    owner: String,
    /// Pixmap files in use, named `<n>-<content hash>.bmp`, so a re-read with the same icon
    /// reuses its file.
    n: u64,
    files: Vec<PathBuf>,
    /// The last properties sent, replayed to the next generation on reload.
    json: String,
}

#[derive(Clone)]
struct Host {
    conn: Connection,
    target: Target,
    items: Arc<Mutex<HashMap<String, Item>>>,
    image_dir: PathBuf,
    seq: Arc<Mutex<u64>>,
}

fn split_key(key: &str) -> (String, String) {
    match key.find('/') {
        Some(i) => (key[..i].to_string(), key[i..].to_string()),
        None => (key.to_string(), "/StatusNotifierItem".to_string()),
    }
}

impl Host {
    async fn connect_to_watcher(&self, host_id: &str) {
        let result = async {
            let w = Proxy::new(&self.conn, WATCHER, WATCHER_PATH, WATCHER).await?;
            w.call::<_, _, ()>("RegisterStatusNotifierHost", &(host_id,)).await?;
            w.get_property::<Vec<String>>("RegisteredStatusNotifierItems").await
        };
        match result.await {
            Ok(items) => {
                for i in items {
                    self.add(i).await;
                }
            }
            Err(e) => quickshell::qs_log!(warn, "quickshell.service.sni", "cannot use the StatusNotifierWatcher: {e}"),
        }
    }

    async fn add(&self, key: String) {
        if self.items.lock().unwrap().contains_key(&key) {
            return;
        }
        let (service, path) = split_key(&key);
        let owner = async {
            let d = zbus::fdo::DBusProxy::new(&self.conn).await?;
            zbus::Result::Ok(d.get_name_owner(zbus::names::BusName::try_from(service.as_str())?).await?)
        };
        let owner = match owner.await {
            Ok(o) => o.to_string(),
            Err(e) => return quickshell::qs_log!(warn, "quickshell.service.sni", "cannot create item for {key}: {e}"),
        };
        let n = {
            let mut s = self.seq.lock().unwrap();
            *s += 1;
            *s
        };
        self.items.lock().unwrap().insert(key.clone(), Item { service, path, owner, n, files: Vec::new(), json: String::new() });
        self.refresh(key);
    }

    fn remove(&self, key: &str) {
        if let Some(item) = self.items.lock().unwrap().remove(key) {
            let key = key.to_string();
            let files = item.files.clone();
            if !queue(&self.target, move |this| {
                this.item_removed(QString::from(&key));
                item.files.iter().for_each(|f| drop(std::fs::remove_file(f)));
            }) {
                files.iter().for_each(|f| drop(std::fs::remove_file(f)));
            }
        }
    }

    fn remove_all(&self) {
        let keys: Vec<String> = self.items.lock().unwrap().keys().cloned().collect();
        keys.iter().for_each(|k| self.remove(k));
    }

    /// Re-reads all properties of an item in its own task, so a slow item can't hold up signals.
    fn refresh(&self, key: String) {
        let host = self.clone();
        spawn(async move {
            let Some((service, path)) = host.items.lock().unwrap().get(&key).map(|i| (i.service.clone(), i.path.clone())) else { return };
            match qs_dbus::get_all(&host.conn, &service, &path, ITEM).await {
                Ok(p) => host.emit(&key, &p),
                Err(e) => quickshell::qs_log!(warn, "quickshell.service.sni", "failed to load tray item {key}: {e}"),
            }
        });
    }

    /// Writes the largest pixmap of an `a(iiay)` list; returns its file URL.
    fn pixmap(&self, n: u64, value: Option<&OwnedValue>, files: &mut Vec<PathBuf>) -> String {
        let Some(list) = value.and_then(|v| Vec::<(i32, i32, Vec<u8>)>::try_from(v.try_clone().ok()?).ok()) else { return String::new() };
        let Some((w, h, data)) = list.iter().max_by_key(|(w, h, _)| w * h) else { return String::new() };
        let Some(p) = Pixmap::from_argb32(*w, *h, data) else { return String::new() };
        let bmp = p.to_bmp();
        let mut hasher = std::hash::DefaultHasher::new();
        std::hash::Hash::hash(&bmp, &mut hasher);
        let file = self.image_dir.join(format!("{n}-{:016x}.bmp", std::hash::Hasher::finish(&hasher)));
        let written = if file.exists() { Ok(()) } else { std::fs::create_dir_all(&self.image_dir).and_then(|_| std::fs::write(&file, bmp)) };
        if let Err(e) = written {
            quickshell::qs_log!(warn, "quickshell.service.sni", "cannot write icon: {e}");
            return String::new();
        }
        let url = format!("file://{}", file.display());
        files.push(file);
        url
    }

    fn emit(&self, key: &str, p: &HashMap<String, OwnedValue>) {
        let s = |k: &str| p.get(k).and_then(|v| String::try_from(v.try_clone().ok()?).ok()).unwrap_or_default();
        let Some(n) = self.items.lock().unwrap().get(key).map(|i| i.n) else { return };
        let mut files = Vec::new();
        let icon = self.pixmap(n, p.get("IconPixmap"), &mut files);
        let attention = self.pixmap(n, p.get("AttentionIconPixmap"), &mut files);
        let has_overlay = p.get("OverlayIconPixmap").is_some_and(|v| matches!(&**v, Value::Array(a) if !a.is_empty()));
        let (tip_title, tip_text) = p
            .get("ToolTip")
            .and_then(|v| <(String, Vec<(i32, i32, Vec<u8>)>, String, String)>::try_from(v.try_clone().ok()?).ok())
            .map(|t| (t.2, t.3))
            .unwrap_or_default();
        let menu = p.get("Menu").and_then(|v| zbus::zvariant::OwnedObjectPath::try_from(v.try_clone().ok()?).ok()).map(|o| o.to_string()).unwrap_or_default();
        let only_menu = p.get("ItemIsMenu").and_then(|v| bool::try_from(&**v).ok()).unwrap_or(false);

        let mut json = String::from("{");
        for (k, v) in [
            ("id", s("Id")),
            ("title", s("Title")),
            ("status", s("Status")),
            ("category", s("Category")),
            ("iconName", s("IconName")),
            ("iconThemePath", s("IconThemePath")),
            ("attentionIconName", s("AttentionIconName")),
            ("overlayIconName", s("OverlayIconName")),
            ("iconPixmap", icon),
            ("attentionIconPixmap", attention),
            ("tooltipTitle", tip_title),
            ("tooltipDescription", tip_text),
            ("menu", menu),
        ] {
            json_string(k, &mut json);
            json.push(':');
            json_string(&v, &mut json);
            json.push(',');
        }
        json.push_str(&format!("\"hasOverlayPixmap\":{has_overlay},\"onlyMenu\":{only_menu}}}"));

        // Replaced icon files are removed only after QML moved to the new ones.
        let stale: Vec<PathBuf> = match self.items.lock().unwrap().get_mut(key) {
            Some(item) => {
                item.json = json.clone();
                std::mem::replace(&mut item.files, files.clone()).into_iter().filter(|f| !files.contains(f)).collect()
            }
            None => return,
        };
        let key = key.to_string();
        queue(&self.target, move |this| {
            this.item_changed(QString::from(&key), QString::from(&json));
            stale.iter().for_each(|f| drop(std::fs::remove_file(f)));
        });
    }

    /// Keys of the items a signal from `owner` at `path` belongs to.
    fn keys_of(&self, owner: &str, path: &str) -> Vec<String> {
        self.items.lock().unwrap().iter().filter(|(_, i)| i.owner == owner && i.path == path).map(|(k, _)| k.clone()).collect()
    }
}

async fn stream(conn: &Connection, iface: &str) -> zbus::Result<zbus::MessageStream> {
    qs_dbus::signals(conn, MatchRule::builder().msg_type(Type::Signal).interface(iface.to_string())?.build()).await
}

async fn run_host(target: Target, items: Arc<Mutex<HashMap<String, Item>>>) -> zbus::Result<()> {
    let conn = Bus::Session.connect().await?;
    let launch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let host_id = format!("org.kde.StatusNotifierHost-{}-{launch}", std::process::id());
    conn.request_name(host_id.as_str()).await?;
    let host = Host { conn: conn.clone(), target, items, image_dir: image_dir("tray"), seq: Default::default() };

    let mut watcher_signals = stream(&conn, WATCHER).await?;
    let mut item_signals = stream(&conn, ITEM).await?;
    let mut watcher_owner = qs_dbus::name_owner_changes(&conn, WATCHER).await?;
    {
        let host = host.clone();
        spawn(async move {
            while let Some(msg) = next(&mut watcher_signals).await {
                let Ok(msg) = msg else { continue };
                let hdr = msg.header();
                let Ok(key) = msg.body().deserialize::<String>() else { continue };
                match hdr.member().map(|m| m.as_str()) {
                    Some("StatusNotifierItemRegistered") => host.add(key).await,
                    Some("StatusNotifierItemUnregistered") => host.remove(&key),
                    _ => {}
                }
            }
        });
    }
    {
        let host = host.clone();
        spawn(async move {
            while let Some(msg) = next(&mut item_signals).await {
                let Ok(msg) = msg else { continue };
                let hdr = msg.header();
                let (Some(sender), Some(path)) = (hdr.sender(), hdr.path()) else { continue };
                if hdr.member().is_some_and(|m| m.starts_with("New")) {
                    host.keys_of(sender.as_str(), path.as_str()).into_iter().for_each(|k| host.refresh(k));
                }
            }
        });
    }
    host.connect_to_watcher(&host_id).await;
    while let Some(msg) = next(&mut watcher_owner).await {
        let Some((_, _, new)) = qs_dbus::owner_change(&msg?) else { continue };
        if new.is_empty() {
            host.remove_all();
        } else {
            host.connect_to_watcher(&host_id).await;
        }
    }
    Ok(())
}

impl cxx_qt::Initialize for qobject::SystemTrayCore {
    fn initialize(self: Pin<&mut Self>) {
        let t = tray();
        let qt = self.qt_thread();
        *t.target.lock().unwrap() = Some((self.token, qt.clone()));
        let mut first = false;
        t.started.call_once(|| first = true);
        if first {
            spawn(async {
                if let Err(e) = run_watcher().await {
                    quickshell::qs_log!(warn, "quickshell.service.sni", "watcher: {e}");
                }
            });
            let (target, items) = (t.target.clone(), t.items.clone());
            spawn(async move {
                if let Err(e) = run_host(target, items).await {
                    quickshell::qs_log!(warn, "quickshell.service.sni", "host: {e}");
                }
            });
            return;
        }
        // A reload: the items the host already knows go to this generation.
        let mut carried: Vec<(String, String)> = t.items.lock().unwrap().iter().filter(|(_, i)| !i.json.is_empty()).map(|(k, i)| (k.clone(), i.json.clone())).collect();
        carried.sort();
        let _ = qt.queue(move |mut this| {
            for (key, json) in carried {
                this.as_mut().item_changed(QString::from(&key), QString::from(&json));
            }
        });
    }
}

impl qobject::SystemTrayCore {
    fn call<B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType + Send + Sync + 'static>(&self, key: &QString, method: &'static str, body: B) {
        let (service, path) = split_key(&key.to_string());
        self.worker.run(format!("SystemTray: {method} on {service}"), move |conn| async move {
            Proxy::new(&conn, service, path, ITEM).await?.call::<_, _, ()>(method, &body).await
        });
    }

    fn activate(&self, key: &QString) {
        self.call(key, "Activate", (0i32, 0i32));
    }

    fn secondary_activate(&self, key: &QString) {
        self.call(key, "SecondaryActivate", (0i32, 0i32));
    }

    fn scroll(&self, key: &QString, delta: i32, horizontal: bool) {
        let orientation = if horizontal { "horizontal" } else { "vertical" };
        self.call(key, "Scroll", (delta, orientation));
    }
}
