//! com.canonical.dbusmenu client (upstream dbus/dbusmenu). One `DBusMenuClient` per open menu;
//! all of them share the session connection, one call worker (in request order) and one signal
//! task that routes LayoutUpdated/ItemsPropertiesUpdated by (unique sender, path), all on the
//! D-Bus thread.
//! Layouts and property updates reach QML as JSON; `icon-data` PNGs become files.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type DBusMenuClient = super::ClientRust;

        #[qinvokable]
        #[cxx_name = "_qsConnect"]
        fn connect_to(self: Pin<&mut DBusMenuClient>, service: &QString, path: &QString);
        /// AboutToShow(id), then the layout below `id` (always, like upstream's prepareToShow).
        #[qinvokable]
        #[cxx_name = "aboutToShow"]
        fn about_to_show(self: &DBusMenuClient, id: i32);
        #[qinvokable]
        #[cxx_name = "updateLayout"]
        fn update_layout(self: &DBusMenuClient, id: i32);
        #[qinvokable]
        #[cxx_name = "sendEvent"]
        fn send_event(self: &DBusMenuClient, id: i32, event: &QString);

        /// `{"id", "props", "children": [...]}` for the subtree that was (re)loaded.
        #[qsignal]
        fn layout(self: Pin<&mut DBusMenuClient>, json: QString);
        /// `{"updated": [[id, props]], "removed": [[id, [names]]]}`.
        #[qsignal]
        #[cxx_name = "propertiesUpdated"]
        fn properties_updated(self: Pin<&mut DBusMenuClient>, json: QString);
        #[qsignal]
        #[cxx_name = "iconThemePath"]
        fn icon_theme_path(self: Pin<&mut DBusMenuClient>, path: QString);
    }

    impl cxx_qt::Threading for DBusMenuClient {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use qs_dbus::{json_string, write_json, Bus, Worker};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use zbus::message::Type;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, MatchRule};

const IFACE: &str = "com.canonical.dbusmenu";

type Qt = cxx_qt::CxxQtThread<qobject::DBusMenuClient>;

struct Route {
    sender: String,
    path: String,
    qt: Qt,
}

struct Shared {
    worker: Worker,
    routes: Mutex<HashMap<u64, Route>>,
}

fn shared() -> Option<&'static Shared> {
    static S: OnceLock<Shared> = OnceLock::new();
    Some(S.get_or_init(|| {
        qs_dbus::rt::spawn(async {
            let res = async { signal_loop(&Bus::Session.connect().await?).await };
            if let Err(e) = res.await {
                quickshell::qs_log!(warn, "quickshell.dbus.dbusmenu", "signal stream failed: {e}");
            }
        });
        Shared { worker: Worker::new(Bus::Session, "DBusMenu"), routes: Mutex::new(HashMap::new()) }
    }))
}

#[derive(Default)]
pub struct ClientRust {
    key: u64,
    service: String,
    path: String,
}

impl Drop for ClientRust {
    fn drop(&mut self) {
        if let Some(s) = shared() {
            s.routes.lock().unwrap().remove(&self.key);
        }
    }
}

fn targets(sender: &str, path: &str) -> Vec<Qt> {
    let Some(s) = shared() else { return Vec::new() };
    s.routes.lock().unwrap().values().filter(|r| r.sender == sender && r.path == path).map(|r| r.qt.clone()).collect()
}

async fn signal_loop(conn: &Connection) -> zbus::Result<()> {
    let rule = MatchRule::builder().msg_type(Type::Signal).interface(IFACE)?.build();
    let mut stream = qs_dbus::signals(conn, rule).await?;
    while let Some(msg) = qs_dbus::rt::next(&mut stream).await {
        let Ok(msg) = msg else { continue };
        let header = msg.header();
        let (Some(sender), Some(path), Some(member)) = (header.sender(), header.path(), header.member()) else { continue };
        let (sender, path) = (sender.to_string(), path.to_string());
        match member.as_str() {
            "LayoutUpdated" => {
                let Ok((_revision, parent)) = msg.body().deserialize::<(u32, i32)>() else { continue };
                // The spec says this is recursive: reload the whole subtree.
                for qt in targets(&sender, &path) {
                    fetch_layout(qt, sender.clone(), path.clone(), parent, false);
                }
            }
            "ItemsPropertiesUpdated" => {
                type Updated = Vec<(i32, HashMap<String, OwnedValue>)>;
                type Removed = Vec<(i32, Vec<String>)>;
                let Ok((updated, removed)) = msg.body().deserialize::<(Updated, Removed)>() else { continue };
                let json = properties_json(&updated, &removed);
                for qt in targets(&sender, &path) {
                    let json = json.clone();
                    let _ = qt.queue(move |obj| obj.properties_updated(QString::from(&json)));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// `icon-data` holds PNG bytes; they are written once per content and referenced by file URL.
fn icon_file(value: &Value) -> Option<String> {
    let Value::Array(a) = value else { return None };
    let bytes: Vec<u8> = a.iter().filter_map(|v| if let Value::U8(b) = v { Some(*b) } else { None }).collect();
    if bytes.is_empty() {
        return Some(String::new());
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    let dir = qs_dbus::pixmap::image_dir("menu");
    let file = dir.join(format!("{:016x}.png", h.finish()));
    if !file.exists() {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&file, &bytes).ok()?;
    }
    Some(format!("file://{}", file.display()))
}

fn props_json(props: &[(&str, &Value)], out: &mut String) {
    out.push('{');
    for (i, &(k, v)) in props.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_string(k, out);
        out.push(':');
        let v = if let Value::Value(inner) = v { &**inner } else { v };
        match (k, icon_file(v)) {
            ("icon-data", Some(url)) => json_string(&url, out),
            _ => write_json(v, out),
        }
    }
    out.push('}');
}

/// One `(ia{sv}av)` layout node as JSON.
fn node_json(id: i32, props: &[(&str, &Value)], children: &[&Value], out: &mut String) {
    out.push_str(&format!("{{\"id\":{id},\"props\":"));
    props_json(props, out);
    out.push_str(",\"children\":[");
    let mut first = true;
    for child in children {
        let child = if let Value::Value(inner) = child { &**inner } else { *child };
        let Value::Structure(s) = child else { continue };
        let [Value::I32(cid), Value::Dict(d), Value::Array(a)] = s.fields() else { continue };
        if !first {
            out.push(',');
        }
        first = false;
        let props: Vec<(&str, &Value)> = d.iter().filter_map(|(k, v)| if let Value::Str(k) = k { Some((k.as_str(), v)) } else { None }).collect();
        let kids: Vec<&Value> = a.iter().collect();
        node_json(*cid, &props, &kids, out);
    }
    out.push_str("]}");
}

fn properties_json(updated: &[(i32, HashMap<String, OwnedValue>)], removed: &[(i32, Vec<String>)]) -> String {
    let mut out = String::from("{\"updated\":[");
    for (i, (id, props)) in updated.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("[{id},"));
        let props: Vec<(&str, &Value)> = props.iter().map(|(k, v)| (k.as_str(), &**v)).collect();
        props_json(&props, &mut out);
        out.push(']');
    }
    out.push_str("],\"removed\":[");
    for (i, (id, names)) in removed.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("[{id},["));
        for (j, n) in names.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            json_string(n, &mut out);
        }
        out.push_str("]]");
    }
    out.push_str("]}");
    out
}

type Layout = (u32, (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>));

fn fetch_layout(qt: Qt, sender: String, path: String, id: i32, about_to_show: bool) {
    let Some(s) = shared() else { return };
    s.worker.run(format!("DBusMenu GetLayout({id}) on {sender}{path}"), move |conn| async move {
        if about_to_show {
            // Errors are ignored like upstream: the layout is requested either way.
            let _ = conn.call_method(Some(sender.as_str()), path.as_str(), Some(IFACE), "AboutToShow", &(id,)).await;
        }
        let reply = conn.call_method(Some(sender.as_str()), path.as_str(), Some(IFACE), "GetLayout", &(id, -1i32, Vec::<String>::new())).await?;
        let (_revision, (root, props, children)): Layout = reply.body().deserialize()?;
        let mut json = String::new();
        let kids: Vec<&Value> = children.iter().map(|v| &**v).collect();
        let props: Vec<(&str, &Value)> = props.iter().map(|(k, v)| (k.as_str(), &**v)).collect();
        node_json(root, &props, &kids, &mut json);
        let _ = qt.queue(move |obj| obj.layout(QString::from(&json)));
        Ok(())
    });
}

fn next_key() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl qobject::DBusMenuClient {
    fn connect_to(mut self: Pin<&mut Self>, service: &QString, path: &QString) {
        let Some(s) = shared() else { return };
        let (service, path) = (service.to_string(), path.to_string());
        let key = next_key();
        let qt = self.qt_thread();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.key = key;
            rust.path = path.clone();
            rust.service = service.clone();
        }
        // Signals carry the unique name, so routes are keyed by the owner of `service`.
        let routes_qt = qt.clone();
        s.worker.run(format!("DBusMenu connect {service}{path}"), move |conn| async move {
            let sender = if service.starts_with(':') {
                service.clone()
            } else {
                zbus::fdo::DBusProxy::new(&conn).await?.get_name_owner(service.as_str().try_into()?).await?.to_string()
            };
            shared().unwrap().routes.lock().unwrap().insert(key, Route { sender, path: path.clone(), qt: routes_qt });
            // Off the shared worker: IconThemePath is optional and an app may never answer it (no timeout on a
            // call), which would hold back the layout of every menu queued behind this one.
            qs_dbus::rt::spawn(async move {
                let Ok(props) = async { zbus::fdo::PropertiesProxy::builder(&conn).destination(service.as_str())?.path(path.as_str())?.build().await }.await else { return };
                let Ok(iface) = IFACE.try_into() else { return };
                if let Ok(v) = props.get(iface, "IconThemePath").await {
                    let dirs: Vec<String> = Vec::<String>::try_from(v).unwrap_or_default();
                    let joined = dirs.join(":");
                    let _ = qt.queue(move |obj| obj.icon_theme_path(QString::from(&joined)));
                }
            });
            Ok(())
        });
    }

    fn about_to_show(&self, id: i32) {
        fetch_layout(self.qt_thread(), self.service.clone(), self.path.clone(), id, true);
    }

    fn update_layout(&self, id: i32) {
        fetch_layout(self.qt_thread(), self.service.clone(), self.path.clone(), id, false);
    }

    fn send_event(&self, id: i32, event: &QString) {
        let Some(s) = shared() else { return };
        let (service, path, event) = (self.service.clone(), self.path.clone(), event.to_string());
        s.worker.run(format!("DBusMenu Event({id}, {event})"), move |conn| async move {
            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0);
            conn.call_method(Some(service.as_str()), path.as_str(), Some(IFACE), "Event", &(id, event.as_str(), Value::from(0i32), ts)).await?;
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_json_nested() {
        let child = Value::from((3i32, HashMap::from([("label", Value::from("_Quit"))]), Vec::<Value>::new()));
        let props: HashMap<String, OwnedValue> = HashMap::from([("children-display".to_string(), Value::from("submenu").try_into().unwrap())]);
        let mut out = String::new();
        let kids = [&Value::Value(Box::new(child))];
        let props: Vec<(&str, &Value)> = props.iter().map(|(k, v)| (k.as_str(), &**v)).collect();
        node_json(0, &props, &kids, &mut out);
        assert_eq!(out, r#"{"id":0,"props":{"children-display":"submenu"},"children":[{"id":3,"props":{"label":"_Quit"},"children":[]}]}"#);
    }

    #[test]
    fn properties_update_json() {
        let updated = vec![(5, HashMap::from([("toggle-state".to_string(), Value::I32(1).try_into().unwrap())]))];
        let removed = vec![(6, vec!["icon-name".to_string()])];
        assert_eq!(properties_json(&updated, &removed), r#"{"updated":[[5,{"toggle-state":1}]],"removed":[[6,["icon-name"]]]}"#);
    }
}
