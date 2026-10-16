//! MPRIS players on the session bus. Three signal streams (owner changes of
//! org.mpris.MediaPlayer2.*, PropertiesChanged and Seeked on /org/mpris/MediaPlayer2) run on
//! their own threads; reads and calls run in order on one worker thread. Players are keyed by their
//! well-known name; signals arrive from unique names and are mapped back.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "MprisCore"]
        #[qml_singleton]
        type MprisCore = super::MprisCoreRust;

        /// Re-reads one property (used for Position, which is never signalled).
        #[qinvokable]
        fn refresh(self: &MprisCore, name: &QString, iface: &QString, prop: &QString);
        /// Calls a method without arguments (Play, Pause, Next, Raise, ...).
        #[qinvokable]
        fn call(self: &MprisCore, name: &QString, iface: &QString, method: &QString);
        #[qinvokable]
        fn seek(self: &MprisCore, name: &QString, offset_us: i64);
        #[qinvokable]
        #[cxx_name = "setPosition"]
        fn set_position(self: &MprisCore, name: &QString, track_id: &QString, position_us: i64);
        #[qinvokable]
        #[cxx_name = "openUri"]
        fn open_uri(self: &MprisCore, name: &QString, uri: &QString);
        #[qinvokable]
        #[cxx_name = "setBool"]
        fn set_bool(self: &MprisCore, name: &QString, iface: &QString, prop: &QString, value: bool);
        #[qinvokable]
        #[cxx_name = "setDouble"]
        fn set_double(self: &MprisCore, name: &QString, iface: &QString, prop: &QString, value: f64);
        #[qinvokable]
        #[cxx_name = "setString"]
        fn set_string(self: &MprisCore, name: &QString, iface: &QString, prop: &QString, value: &QString);

        /// A player's properties for one interface, as JSON. The first one for both
        /// interfaces makes the player ready.
        #[qsignal]
        #[cxx_name = "propertiesChanged"]
        fn properties_changed(self: Pin<&mut MprisCore>, name: QString, iface: QString, json: QString);
        #[qsignal]
        fn seeked(self: Pin<&mut MprisCore>, name: QString, position_us: i64);
        #[qsignal]
        #[cxx_name = "playerRemoved"]
        fn player_removed(self: Pin<&mut MprisCore>, name: QString);
    }

    impl cxx_qt::Initialize for MprisCore {}
    impl cxx_qt::Threading for MprisCore {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtThread, Threading};
use cxx_qt_lib::QString;
use qs_dbus::zbus::{self, blocking::Connection, blocking::MessageIterator, blocking::Proxy, message::Type, MatchRule};
use qs_dbus::{props_json, SetValue};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use zbus::zvariant::{ObjectPath, OwnedValue};

const PREFIX: &str = "org.mpris.MediaPlayer2";
const PATH: &str = "/org/mpris/MediaPlayer2";
const IFACES: [&str; 2] = ["org.mpris.MediaPlayer2", "org.mpris.MediaPlayer2.Player"];

type Qt = CxxQtThread<qobject::MprisCore>;

#[derive(Default)]
pub struct MprisCoreRust {
    worker: Arc<OnceLock<qs_dbus::Worker>>,
}

#[derive(Clone, Default)]
struct Owners(Arc<Mutex<HashMap<String, String>>>);

impl Owners {
    fn name_of(&self, unique: &str) -> Option<String> {
        self.0.lock().unwrap().get(unique).cloned()
    }
}

fn emit_props(qt: &Qt, name: &str, iface: &str, props: &HashMap<String, OwnedValue>) {
    let (name, iface, json) = (name.to_string(), iface.to_string(), props_json(props.iter().map(|(k, v)| (k, &**v))));
    let _ = qt.queue(move |this| this.properties_changed(QString::from(&name), QString::from(&iface), QString::from(&json)));
}

/// GetAll of both interfaces for a newly seen player.
fn add_player(conn: Connection, qt: Qt, owners: Owners, name: String, unique: String) {
    std::thread::spawn(move || {
        owners.0.lock().unwrap().insert(unique, name.clone());
        for iface in IFACES {
            let props: zbus::Result<HashMap<String, OwnedValue>> =
                Proxy::new(&conn, name.clone(), PATH, "org.freedesktop.DBus.Properties").and_then(|p| p.call("GetAll", &(iface,)));
            match props {
                Ok(p) => emit_props(&qt, &name, iface, &p),
                Err(e) => return eprintln!("Mpris: cannot read {name}: {e}"),
            }
        }
    });
}

fn stream(conn: &Connection, rule: zbus::Result<MatchRule<'static>>) -> zbus::Result<MessageIterator> {
    MessageIterator::for_match_rule(rule?, conn, None)
}

fn run(conn: Connection, qt: Qt) -> zbus::Result<()> {
    let owners = Owners::default();
    let changes = stream(
        &conn,
        MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg0ns(PREFIX)
            .map(|b| b.build()),
    )?;
    let props = stream(
        &conn,
        MatchRule::builder().msg_type(Type::Signal).path(PATH)?.interface("org.freedesktop.DBus.Properties")?.member("PropertiesChanged").map(|b| b.build()),
    )?;
    let seeks = stream(
        &conn,
        MatchRule::builder().msg_type(Type::Signal).path(PATH)?.interface("org.mpris.MediaPlayer2.Player")?.member("Seeked").map(|b| b.build()),
    )?;

    let dbus = zbus::blocking::fdo::DBusProxy::new(&conn)?;
    for name in dbus.list_names()? {
        if name.starts_with(PREFIX) {
            if let Ok(owner) = dbus.get_name_owner(name.as_ref()) {
                add_player(conn.clone(), qt.clone(), owners.clone(), name.to_string(), owner.to_string());
            }
        }
    }

    {
        let (qt, owners) = (qt.clone(), owners.clone());
        std::thread::spawn(move || {
            for msg in props.flatten() {
                let Some(unique) = msg.header().sender().map(|s| s.to_string()) else { continue };
                let Some(name) = owners.name_of(&unique) else { continue };
                if let Ok((iface, changed, _)) = msg.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>() {
                    emit_props(&qt, &name, &iface, &changed);
                }
            }
        });
    }
    {
        let (qt, owners) = (qt.clone(), owners.clone());
        std::thread::spawn(move || {
            for msg in seeks.flatten() {
                let Some(name) = msg.header().sender().and_then(|s| owners.name_of(s.as_str())) else { continue };
                if let Ok(pos) = msg.body().deserialize::<i64>() {
                    let _ = qt.queue(move |this| this.seeked(QString::from(&name), pos));
                }
            }
        });
    }
    for msg in changes {
        let msg = msg?;
        let Ok((name, old, new)) = msg.body().deserialize::<(String, String, String)>() else { continue };
        if !name.starts_with(PREFIX) {
            continue;
        }
        if !old.is_empty() {
            owners.0.lock().unwrap().remove(&old);
            let n = name.clone();
            let _ = qt.queue(move |this| this.player_removed(QString::from(&n)));
        }
        if !new.is_empty() {
            add_player(conn.clone(), qt.clone(), owners.clone(), name, new);
        }
    }
    Ok(())
}

impl cxx_qt::Initialize for qobject::MprisCore {
    fn initialize(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        let slot = self.worker.clone();
        std::thread::Builder::new()
            .name("mpris".into())
            .spawn(move || {
                let conn = match Connection::session() {
                    Ok(c) => {
                        slot.get_or_init(|| qs_dbus::Worker::new(c.clone(), "mpris-calls"));
                        c
                    }
                    Err(e) => return eprintln!("Mpris: no session bus: {e}"),
                };
                if let Err(e) = run(conn, qt) {
                    eprintln!("Mpris: {e}");
                }
            })
            .expect("spawn mpris thread");
    }
}

impl qobject::MprisCore {
    /// Queues `f` on the call worker, in request order.
    fn with_conn(&self, what: String, f: impl FnOnce(&Connection) -> zbus::Result<()> + Send + 'static) {
        if let Some(worker) = self.worker.get() {
            worker.run(format!("Mpris: {what}"), f);
        }
    }

    fn refresh(&self, name: &QString, iface: &QString, prop: &QString) {
        let (name, iface, prop) = (name.to_string(), iface.to_string(), prop.to_string());
        let qt = self.qt_thread();
        self.with_conn(format!("reading {prop} of {name}"), move |conn| {
            let p = Proxy::new(conn, name.clone(), PATH, "org.freedesktop.DBus.Properties")?;
            let value: OwnedValue = p.call("Get", &(iface.as_str(), prop.as_str()))?;
            emit_props(&qt, &name, &iface, &HashMap::from([(prop, value)]));
            Ok(())
        });
    }

    fn call(&self, name: &QString, iface: &QString, method: &QString) {
        let (name, iface, method) = (name.to_string(), iface.to_string(), method.to_string());
        self.with_conn(format!("{method} on {name}"), move |conn| Proxy::new(conn, name, PATH, iface)?.call(method.as_str(), &()));
    }

    fn seek(&self, name: &QString, offset_us: i64) {
        let name = name.to_string();
        self.with_conn(format!("Seek on {name}"), move |conn| Proxy::new(conn, name, PATH, IFACES[1])?.call("Seek", &(offset_us,)));
    }

    fn set_position(&self, name: &QString, track_id: &QString, position_us: i64) {
        let (name, track) = (name.to_string(), track_id.to_string());
        self.with_conn(format!("SetPosition on {name}"), move |conn| {
            Proxy::new(conn, name, PATH, IFACES[1])?.call("SetPosition", &(ObjectPath::try_from(track.as_str())?, position_us))
        });
    }

    fn open_uri(&self, name: &QString, uri: &QString) {
        let (name, uri) = (name.to_string(), uri.to_string());
        self.with_conn(format!("OpenUri on {name}"), move |conn| Proxy::new(conn, name, PATH, IFACES[1])?.call("OpenUri", &(uri.as_str(),)));
    }

    fn set(&self, name: &QString, iface: &QString, prop: &QString, value: SetValue) {
        let (name, iface, prop) = (name.to_string(), iface.to_string(), prop.to_string());
        self.with_conn(format!("setting {prop} on {name}"), move |conn| qs_dbus::set_property(conn, &name, PATH, &iface, &prop, value));
    }

    fn set_bool(&self, name: &QString, iface: &QString, prop: &QString, value: bool) {
        self.set(name, iface, prop, SetValue::Bool(value));
    }

    fn set_double(&self, name: &QString, iface: &QString, prop: &QString, value: f64) {
        self.set(name, iface, prop, SetValue::F64(value));
    }

    fn set_string(&self, name: &QString, iface: &QString, prop: &QString, value: &QString) {
        self.set(name, iface, prop, SetValue::Str(value.to_string()));
    }
}

