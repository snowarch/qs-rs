//! UPower and power-profiles-daemon clients on the system bus. Each core is one task on the D-Bus
//! thread: initial GetAll, then a signal stream; properties reach QML as JSON objects.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "UPowerCore"]
        #[qml_singleton]
        type UPowerCore = super::UPowerCoreRust;

        /// `path` is "/" for the UPower object itself (OnBattery, ...).
        #[qsignal]
        #[cxx_name = "propertiesChanged"]
        fn properties_changed(self: Pin<&mut UPowerCore>, path: QString, json: QString);
        #[qsignal]
        #[cxx_name = "displayDevice"]
        fn display_device(self: Pin<&mut UPowerCore>, path: QString);
        #[qsignal]
        #[cxx_name = "deviceRemoved"]
        fn device_removed(self: Pin<&mut UPowerCore>, path: QString);

        #[qobject]
        #[qml_element = "PowerProfilesCore"]
        #[qml_singleton]
        type PowerProfilesCore = super::PowerProfilesCoreRust;

        #[qinvokable]
        #[cxx_name = "setActiveProfile"]
        fn set_active_profile(self: &PowerProfilesCore, profile: &QString);
        #[qsignal]
        #[cxx_name = "propertiesChanged"]
        fn properties_changed(self: Pin<&mut PowerProfilesCore>, json: QString);
    }

    impl cxx_qt::Initialize for UPowerCore {}
    impl cxx_qt::Threading for UPowerCore {}
    impl cxx_qt::Initialize for PowerProfilesCore {}
    impl cxx_qt::Threading for PowerProfilesCore {}
}

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use qs_dbus::rt::next;
use qs_dbus::zbus::{self, Proxy};
use qs_dbus::{get_all, props_json, signals_from, Bus};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const UPOWER: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
const DEVICE_IFACE: &str = "org.freedesktop.UPower.Device";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

#[derive(Default)]
pub struct UPowerCoreRust;

fn json_of(props: &HashMap<String, OwnedValue>) -> String {
    props_json(props.iter().map(|(k, v)| (k, &**v)))
}

impl cxx_qt::Initialize for qobject::UPowerCore {
    fn initialize(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        qs_dbus::rt::spawn(async move {
            let props = |path: String, json: String| {
                let _ = qt.queue(move |this| this.properties_changed(QString::from(&path), QString::from(&json)));
            };
            let run = async {
                let conn = Bus::System.connect().await?;
                let mut stream = signals_from(&conn, UPOWER).await?;
                let fetch = |path: String| {
                    let conn = conn.clone();
                    async move {
                        match get_all(&conn, UPOWER, &path, DEVICE_IFACE).await {
                            Ok(p) => props(path, json_of(&p)),
                            Err(e) => quickshell::qs_log!(warn, "quickshell.service.upower", "could not read {path}: {e}"),
                        }
                    }
                };
                props("/".into(), json_of(&get_all(&conn, UPOWER, UPOWER_PATH, UPOWER).await?));
                let upower = Proxy::new(&conn, UPOWER, UPOWER_PATH, UPOWER).await?;
                let display: OwnedObjectPath = upower.call("GetDisplayDevice", &()).await?;
                fetch(display.to_string()).await;
                let path = display.to_string();
                let _ = qt.queue(move |this| this.display_device(QString::from(&path)));
                let devices: Vec<OwnedObjectPath> = upower.call("EnumerateDevices", &()).await?;
                for d in &devices {
                    fetch(d.to_string()).await;
                }
                while let Some(msg) = next(&mut stream).await {
                    let msg = msg?;
                    let header = msg.header();
                    let (Some(member), Some(path)) = (header.member(), header.path()) else { continue };
                    match member.as_str() {
                        "PropertiesChanged" => {
                            let Ok((iface, changed, _)) = msg.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>() else { continue };
                            let key = if iface == UPOWER { "/".to_string() } else { path.to_string() };
                            props(key, json_of(&changed));
                        }
                        "DeviceAdded" => {
                            if let Ok(p) = msg.body().deserialize::<OwnedObjectPath>() {
                                fetch(p.to_string()).await;
                            }
                        }
                        "DeviceRemoved" => {
                            if let Ok(p) = msg.body().deserialize::<OwnedObjectPath>() {
                                let p = p.to_string();
                                let _ = qt.queue(move |this| this.device_removed(QString::from(&p)));
                            }
                        }
                        _ => {}
                    }
                }
                zbus::Result::Ok(())
            };
            if let Err(e) = run.await {
                quickshell::qs_log!(warn, "quickshell.service.upower", "{e}");
            }
        });
    }
}

/// power-profiles-daemon: the UPower name first, the legacy net.hadess name as fallback.
const PROFILES: [(&str, &str); 2] = [
    ("org.freedesktop.UPower.PowerProfiles", "/org/freedesktop/UPower/PowerProfiles"),
    ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles"),
];

#[derive(Default)]
pub struct PowerProfilesCoreRust {
    /// (service, path) once connected.
    target: Arc<Mutex<Option<(&'static str, &'static str)>>>,
    /// A profile asked for before the daemon answered: set as soon as it does (a shell restoring its profile at start
    /// asks within the first milliseconds).
    pending: Arc<Mutex<Option<String>>>,
}

async fn set_profile(conn: &zbus::Connection, service: &'static str, path: &'static str, profile: &str) -> zbus::Result<()> {
    let p = Proxy::new(conn, service, path, PROPS_IFACE).await?;
    p.call::<_, _, ()>("Set", &(service, "ActiveProfile", Value::from(profile))).await
}

impl cxx_qt::Initialize for qobject::PowerProfilesCore {
    fn initialize(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        let target = self.target.clone();
        let pending = self.pending.clone();
        qs_dbus::rt::spawn(async move {
            let emit = |json: String| {
                let _ = qt.queue(move |this| this.properties_changed(QString::from(&json)));
            };
            let run = async {
                let conn = Bus::System.connect().await?;
                let mut last = None;
                for (service, path) in PROFILES {
                    let mut stream = signals_from(&conn, service).await?;
                    match get_all(&conn, service, path, service).await {
                        Ok(props) => {
                            *target.lock().unwrap() = Some((service, path));
                            emit(json_of(&props));
                            let asked = pending.lock().unwrap().take();
                            if let Some(profile) = asked {
                                if let Err(e) = set_profile(&conn, service, path, &profile).await {
                                    quickshell::qs_log!(warn, "quickshell.service.powerprofiles", "could not set the profile to {profile}: {e}");
                                }
                            }
                            while let Some(msg) = next(&mut stream).await {
                                let msg = msg?;
                                if msg.header().member().map(|m| m.as_str()) != Some("PropertiesChanged") {
                                    continue;
                                }
                                if let Ok((_, changed, _)) = msg.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>() {
                                    emit(json_of(&changed));
                                }
                            }
                            return Ok(());
                        }
                        Err(e) => last = Some(e),
                    }
                }
                Err(last.unwrap_or(zbus::Error::Unsupported))
            };
            if let Err(e) = run.await {
                // no daemon on this system: upstream's one line, so both runtimes log the same kind
                if e.to_string().starts_with("org.freedesktop.DBus.Error.ServiceUnknown") {
                    quickshell::qs_log!(warn, "quickshell.service.powerprofiles", "Could not start PowerProfilesDaemon. The PowerProfiles service will not work.");
                } else {
                    quickshell::qs_log!(warn, "quickshell.service.powerprofiles", "{e}");
                }
            }
        });
    }
}

impl qobject::PowerProfilesCore {
    fn set_active_profile(&self, profile: &QString) {
        let profile = profile.to_string();
        // Held under the target's lock, so the connecting task either sees it pending or this call sees the target.
        let target = self.target.lock().unwrap();
        let Some((service, path)) = *target else {
            *self.pending.lock().unwrap() = Some(profile);
            return;
        };
        drop(target);
        qs_dbus::rt::spawn(async move {
            let res = async {
                let conn = Bus::System.connect().await?;
                set_profile(&conn, service, path, &profile).await
            };
            if let Err(e) = res.await {
                quickshell::qs_log!(warn, "quickshell.service.powerprofiles", "could not set the profile to {profile}: {e}");
            }
        });
    }
}
