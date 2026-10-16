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
use qs_dbus::rt::{next, spawn};
use qs_dbus::zbus::{self, message::Type, Connection, MatchRule, Proxy};
use qs_dbus::{get_all, props_json, signals, Bus, SetValue};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::zvariant::{ObjectPath, OwnedValue};

const PREFIX: &str = "org.mpris.MediaPlayer2";
const PATH: &str = "/org/mpris/MediaPlayer2";
const IFACES: [&str; 2] = ["org.mpris.MediaPlayer2", "org.mpris.MediaPlayer2.Player"];

type Qt = CxxQtThread<qobject::MprisCore>;

pub struct MprisCoreRust {
    worker: qs_dbus::Worker,
}

impl Default for MprisCoreRust {
    fn default() -> Self {
        Self { worker: qs_dbus::Worker::new(Bus::Session, "Mpris") }
    }
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
    owners.0.lock().unwrap().insert(unique, name.clone());
    spawn(async move {
        for iface in IFACES {
            match get_all(&conn, &name, PATH, iface).await {
                Ok(p) => emit_props(&qt, &name, iface, &p),
                Err(e) => return quickshell::qs_log!(warn, "quickshell.service.mpris.watcher", "cannot read {name}: {e}"),
            }
        }
    });
}

async fn run(conn: Connection, qt: Qt) -> zbus::Result<()> {
    let owners = Owners::default();
    let rule = |b: zbus::Result<zbus::match_rule::Builder<'static>>| b.map(|b| b.build());
    let mut changes = signals(
        &conn,
        rule(MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg0ns(PREFIX))?,
    )
    .await?;
    let mut props = signals(
        &conn,
        rule(MatchRule::builder().msg_type(Type::Signal).path(PATH)?.interface("org.freedesktop.DBus.Properties")?.member("PropertiesChanged"))?,
    )
    .await?;
    let mut seeks = signals(
        &conn,
        rule(MatchRule::builder().msg_type(Type::Signal).path(PATH)?.interface("org.mpris.MediaPlayer2.Player")?.member("Seeked"))?,
    )
    .await?;

    let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
    for name in dbus.list_names().await? {
        if name.starts_with(PREFIX) {
            if let Ok(owner) = dbus.get_name_owner(name.as_ref()).await {
                add_player(conn.clone(), qt.clone(), owners.clone(), name.to_string(), owner.to_string());
            }
        }
    }

    {
        let (qt, owners) = (qt.clone(), owners.clone());
        spawn(async move {
            while let Some(msg) = next(&mut props).await {
                let Ok(msg) = msg else { continue };
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
        spawn(async move {
            while let Some(msg) = next(&mut seeks).await {
                let Ok(msg) = msg else { continue };
                let Some(name) = msg.header().sender().and_then(|s| owners.name_of(s.as_str())) else { continue };
                if let Ok(pos) = msg.body().deserialize::<i64>() {
                    let _ = qt.queue(move |this| this.seeked(QString::from(&name), pos));
                }
            }
        });
    }
    while let Some(msg) = next(&mut changes).await {
        let Some((name, old, new)) = qs_dbus::owner_change(&msg?) else { continue };
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
        spawn(async move {
            let conn = match Bus::Session.connect().await {
                Ok(c) => c,
                Err(e) => return quickshell::qs_log!(warn, "quickshell.service.mpris.watcher", "no session bus: {e}"),
            };
            if let Err(e) = run(conn, qt).await {
                quickshell::qs_log!(warn, "quickshell.service.mpris.watcher", "{e}");
            }
        });
    }
}

impl qobject::MprisCore {
    /// Queues `f` on the call worker, in request order.
    fn with_conn<F>(&self, what: String, f: impl FnOnce(Connection) -> F + Send + 'static)
    where
        F: std::future::Future<Output = zbus::Result<()>> + Send + 'static,
    {
        self.worker.run(format!("Mpris: {what}"), f);
    }

    fn refresh(&self, name: &QString, iface: &QString, prop: &QString) {
        let (name, iface, prop) = (name.to_string(), iface.to_string(), prop.to_string());
        let qt = self.qt_thread();
        self.with_conn(format!("reading {prop} of {name}"), move |conn| async move {
            let p = Proxy::new(&conn, name.clone(), PATH, "org.freedesktop.DBus.Properties").await?;
            let value: OwnedValue = p.call("Get", &(iface.as_str(), prop.as_str())).await?;
            emit_props(&qt, &name, &iface, &HashMap::from([(prop, value)]));
            Ok(())
        });
    }

    fn call(&self, name: &QString, iface: &QString, method: &QString) {
        let (name, iface, method) = (name.to_string(), iface.to_string(), method.to_string());
        self.with_conn(format!("{method} on {name}"), move |conn| async move {
            Proxy::new(&conn, name, PATH, iface).await?.call(method.as_str(), &()).await
        });
    }

    fn seek(&self, name: &QString, offset_us: i64) {
        let name = name.to_string();
        self.with_conn(format!("Seek on {name}"), move |conn| async move {
            Proxy::new(&conn, name, PATH, IFACES[1]).await?.call("Seek", &(offset_us,)).await
        });
    }

    fn set_position(&self, name: &QString, track_id: &QString, position_us: i64) {
        let (name, track) = (name.to_string(), track_id.to_string());
        self.with_conn(format!("SetPosition on {name}"), move |conn| async move {
            let track = ObjectPath::try_from(track.as_str())?;
            Proxy::new(&conn, name, PATH, IFACES[1]).await?.call("SetPosition", &(track, position_us)).await
        });
    }

    fn open_uri(&self, name: &QString, uri: &QString) {
        let (name, uri) = (name.to_string(), uri.to_string());
        self.with_conn(format!("OpenUri on {name}"), move |conn| async move {
            Proxy::new(&conn, name, PATH, IFACES[1]).await?.call("OpenUri", &(uri.as_str(),)).await
        });
    }

    fn set(&self, name: &QString, iface: &QString, prop: &QString, value: SetValue) {
        let (name, iface, prop) = (name.to_string(), iface.to_string(), prop.to_string());
        self.with_conn(format!("setting {prop} on {name}"), move |conn| async move {
            qs_dbus::set_property(&conn, &name, PATH, &iface, &prop, value).await
        });
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

