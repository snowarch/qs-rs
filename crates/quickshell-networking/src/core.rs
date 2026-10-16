//! NetworkManager on the system bus: its object manager stream (devices, access points, active
//! connections) feeds QML as JSON, saved profiles are read on request, and every write or method
//! call runs in order on one worker so the Qt thread never waits on the bus.

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
        #[qml_element = "NmCore"]
        #[qml_singleton]
        type NmCore = super::NmCoreRust;

        #[qinvokable]
        #[cxx_name = "setBool"]
        fn set_bool(self: &NmCore, path: &QString, iface: &QString, name: &QString, value: bool);
        /// A method without arguments whose reply is ignored (Disconnect, Delete, ClearSecrets,
        /// CheckConnectivity).
        #[qinvokable]
        fn call(self: &NmCore, path: &QString, iface: &QString, method: &QString);
        #[qinvokable]
        #[cxx_name = "requestScan"]
        fn request_scan(self: &NmCore, device: &QString);
        #[qinvokable]
        fn activate(self: &NmCore, connection: &QString, device: &QString, specific: &QString);
        /// AddAndActivateConnection with no settings, or only a PSK when `psk` is not empty.
        #[qinvokable]
        #[cxx_name = "addAndActivate"]
        fn add_and_activate(self: &NmCore, device: &QString, specific: &QString, psk: &QString);
        /// Writes a PSK into a saved profile, then activates it on `device`.
        #[qinvokable]
        #[cxx_name = "updatePskAndActivate"]
        fn update_psk_and_activate(self: &NmCore, connection: &QString, device: &QString, psk: &QString);
        /// GetSettings; answers through `settingsLoaded`.
        #[qinvokable]
        #[cxx_name = "loadSettings"]
        fn load_settings(self: &NmCore, path: &QString);
        /// upstream findBestWirelessSecurity
        #[qinvokable]
        fn security(self: &NmCore, caps: u32, adhoc: bool, flags: u32, wpa: u32, rsn: u32) -> i32;
        /// CLOCK_BOOTTIME in ms, the clock of Device.Wireless.LastScan.
        #[qinvokable]
        #[cxx_name = "bootTimeMs"]
        fn boot_time_ms(self: &NmCore) -> f64;

        #[qsignal]
        fn ready(self: Pin<&mut NmCore>);
        #[qsignal]
        #[cxx_name = "interfacesAdded"]
        fn interfaces_added(self: Pin<&mut NmCore>, path: QString, json: QString);
        #[qsignal]
        #[cxx_name = "interfacesRemoved"]
        fn interfaces_removed(self: Pin<&mut NmCore>, path: QString, interfaces: QStringList);
        #[qsignal]
        #[cxx_name = "propertiesChanged"]
        fn properties_changed(self: Pin<&mut NmCore>, path: QString, iface: QString, json: QString);
        /// Device.StateChanged and Connection.Active.StateChanged: new state and reason.
        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut NmCore>, path: QString, iface: QString, state: u32, reason: u32);
        #[qsignal]
        #[cxx_name = "settingsUpdated"]
        fn settings_updated(self: Pin<&mut NmCore>, path: QString);
        #[qsignal]
        #[cxx_name = "settingsLoaded"]
        fn settings_loaded(self: Pin<&mut NmCore>, path: QString, json: QString);
        #[qsignal]
        #[cxx_name = "serviceLost"]
        fn service_lost(self: Pin<&mut NmCore>);
    }

    impl cxx_qt::Initialize for NmCore {}
    impl cxx_qt::Threading for NmCore {}
}

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::{QString, QStringList};
use qs_dbus::object_manager::{self, Event};
use qs_dbus::{Bus, SetValue};
use std::collections::HashMap;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";

type SettingsMap = HashMap<String, HashMap<String, OwnedValue>>;

pub struct NmCoreRust {
    worker: qs_dbus::Worker,
}

impl Default for NmCoreRust {
    fn default() -> Self {
        Self { worker: qs_dbus::Worker::new(Bus::System, "Networking") }
    }
}

fn settings_json(map: &SettingsMap) -> String {
    let mut out = String::from("{");
    for (i, (group, props)) in map.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        qs_dbus::json_string(group, &mut out);
        out.push(':');
        out.push_str(&qs_dbus::props_json(props.iter().map(|(k, v)| (k, &**v))));
    }
    out.push('}');
    out
}

impl cxx_qt::Initialize for qobject::NmCore {
    fn initialize(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        qs_dbus::rt::spawn(async move {
            let conn = match Bus::System.connect().await {
                Ok(c) => c,
                Err(e) => return quickshell::qs_log!(warn, "quickshell.network", "no system bus: {e}"),
            };
            let announced = std::sync::atomic::AtomicBool::new(false);
            let res = object_manager::run(&conn, NM, "/org/freedesktop", |event| {
                if !announced.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    let _ = qt.queue(|this| this.ready());
                }
                let _ = match event {
                    Event::InterfacesAdded(path, json) => qt.queue(move |this| this.interfaces_added(QString::from(&path), QString::from(&json))),
                    Event::InterfacesRemoved(path, ifaces) => qt.queue(move |this| {
                        let mut list = QStringList::default();
                        for i in &ifaces {
                            list.append(QString::from(i));
                        }
                        this.interfaces_removed(QString::from(&path), list)
                    }),
                    Event::PropertiesChanged(path, iface, json) => {
                        qt.queue(move |this| this.properties_changed(QString::from(&path), QString::from(&iface), QString::from(&json)))
                    }
                    Event::Signal(msg) => {
                        let header = msg.header();
                        let (Some(path), Some(iface), Some(member)) = (header.path(), header.interface(), header.member()) else { return };
                        let (path, iface) = (path.to_string(), iface.to_string());
                        match member.as_str() {
                            "StateChanged" if iface == "org.freedesktop.NetworkManager.Device" => {
                                let Ok((state, _old, reason)) = msg.body().deserialize::<(u32, u32, u32)>() else { return };
                                qt.queue(move |this| this.state_changed(QString::from(&path), QString::from(&iface), state, reason))
                            }
                            "StateChanged" if iface == "org.freedesktop.NetworkManager.Connection.Active" => {
                                let Ok((state, reason)) = msg.body().deserialize::<(u32, u32)>() else { return };
                                qt.queue(move |this| this.state_changed(QString::from(&path), QString::from(&iface), state, reason))
                            }
                            "Updated" => qt.queue(move |this| this.settings_updated(QString::from(&path))),
                            _ => return,
                        }
                    }
                };
            })
            .await;
            match res {
                // GetManagedObjects fails when NetworkManager is not running: no backend.
                Err(e) if !announced.load(std::sync::atomic::Ordering::Relaxed) => quickshell::qs_log!(info, "quickshell.network", "NetworkManager not available: {e}"),
                Err(e) => quickshell::qs_log!(warn, "quickshell.network", "{e}"),
                Ok(()) => {}
            }
            let _ = qt.queue(|this| this.service_lost());
        });
    }
}

fn path(p: &str) -> zbus::Result<ObjectPath<'_>> {
    Ok(ObjectPath::try_from(if p.is_empty() { "/" } else { p })?)
}

async fn proxy<'a>(conn: &zbus::Connection, obj: &str, iface: &str) -> zbus::Result<zbus::Proxy<'a>> {
    zbus::Proxy::new(conn, NM, obj.to_string(), iface.to_string()).await
}

impl qobject::NmCore {
    fn set_bool(&self, path: &QString, iface: &QString, name: &QString, value: bool) {
        let (path, iface, name) = (path.to_string(), iface.to_string(), name.to_string());
        self.worker.run(format!("Networking: setting {name} on {path}"), move |conn| async move {
            qs_dbus::set_property(&conn, NM, &path, &iface, &name, SetValue::Bool(value)).await
        });
    }

    fn call(&self, path: &QString, iface: &QString, method: &QString) {
        let (path, iface, method) = (path.to_string(), iface.to_string(), method.to_string());
        self.worker.run(format!("Networking: {method} on {path}"), move |conn| async move {
            proxy(&conn, &path, &iface).await?.call_method(method.as_str(), &()).await.map(drop)
        });
    }

    fn request_scan(&self, device: &QString) {
        let device = device.to_string();
        self.worker.run(format!("Networking: scan on {device}"), move |conn| async move {
            let opts: HashMap<&str, Value> = HashMap::new();
            proxy(&conn, &device, "org.freedesktop.NetworkManager.Device.Wireless").await?.call_method("RequestScan", &(opts,)).await.map(drop)
        });
    }

    fn activate(&self, connection: &QString, device: &QString, specific: &QString) {
        let (c, d, s) = (connection.to_string(), device.to_string(), specific.to_string());
        self.worker.run(format!("Networking: activating {c} on {d}"), move |conn| async move {
            proxy(&conn, NM_PATH, NM).await?.call_method("ActivateConnection", &(path(&c)?, path(&d)?, path(&s)?)).await.map(drop)
        });
    }

    fn add_and_activate(&self, device: &QString, specific: &QString, psk: &QString) {
        let (d, s, psk) = (device.to_string(), specific.to_string(), psk.to_string());
        self.worker.run(format!("Networking: new connection on {d}"), move |conn| async move {
            let mut settings: HashMap<&str, HashMap<&str, Value>> = HashMap::new();
            if !psk.is_empty() {
                settings.entry("802-11-wireless-security").or_default().insert("psk", Value::from(psk.as_str()));
            }
            proxy(&conn, NM_PATH, NM).await?.call_method("AddAndActivateConnection", &(settings, path(&d)?, path(&s)?)).await.map(drop)
        });
    }

    fn update_psk_and_activate(&self, connection: &QString, device: &QString, psk: &QString) {
        let (c, d, psk) = (connection.to_string(), device.to_string(), psk.to_string());
        self.worker.run(format!("Networking: writing a PSK into {c}"), move |conn| async move {
            const IFACE: &str = "org.freedesktop.NetworkManager.Settings.Connection";
            let mut map: SettingsMap = proxy(&conn, &c, IFACE).await?.call_method("GetSettings", &()).await?.body().deserialize()?;
            map.entry("802-11-wireless-security".into())
                .or_default()
                .insert("psk".into(), OwnedValue::try_from(Value::from(psk.as_str()))?);
            proxy(&conn, &c, IFACE).await?.call_method("Update", &(map,)).await?;
            proxy(&conn, NM_PATH, NM).await?.call_method("ActivateConnection", &(path(&c)?, path(&d)?, path("/")?)).await.map(drop)
        });
    }

    fn load_settings(&self, path: &QString) {
        let p = path.to_string();
        let qt = self.qt_thread();
        self.worker.run(format!("Networking: reading {p}"), move |conn| async move {
            let reply = proxy(&conn, &p, "org.freedesktop.NetworkManager.Settings.Connection").await?.call_method("GetSettings", &()).await?;
            let map: SettingsMap = reply.body().deserialize()?;
            let json = settings_json(&map);
            let _ = qt.queue(move |this| this.settings_loaded(QString::from(&p), QString::from(&json)));
            Ok(())
        });
    }

    fn security(&self, caps: u32, adhoc: bool, flags: u32, wpa: u32, rsn: u32) -> i32 {
        crate::security::best(caps, adhoc, flags, wpa, rsn)
    }

    fn boot_time_ms(&self) -> f64 {
        let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
        unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
        ts.tv_sec as f64 * 1000.0 + ts.tv_nsec as f64 / 1e6
    }
}
