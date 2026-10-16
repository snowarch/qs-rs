//! UPower and power-profiles-daemon clients on the system bus. Each core runs one thread:
//! initial GetAll, then a signal stream; properties reach QML as JSON objects.

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
use qs_dbus::props_json;
use qs_dbus::zbus::{self, blocking::Connection, blocking::MessageIterator, blocking::Proxy, message::Type, MatchRule};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const UPOWER: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
const DEVICE_IFACE: &str = "org.freedesktop.UPower.Device";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

#[derive(Default)]
pub struct UPowerCoreRust;

fn get_all(conn: &Connection, service: &str, path: &str, iface: &str) -> zbus::Result<HashMap<String, OwnedValue>> {
    let proxy = Proxy::new(conn, service.to_string(), path.to_string(), PROPS_IFACE)?;
    proxy.call("GetAll", &(iface,))
}

fn json_of(props: &HashMap<String, OwnedValue>) -> String {
    props_json(props.iter().map(|(k, v)| (k, &**v)))
}

fn signals(conn: &Connection, sender: &str) -> zbus::Result<MessageIterator> {
    let rule = MatchRule::builder().msg_type(Type::Signal).sender(sender.to_string())?.build();
    MessageIterator::for_match_rule(rule, conn, None)
}

impl cxx_qt::Initialize for qobject::UPowerCore {
    fn initialize(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        std::thread::Builder::new()
            .name("upower".into())
            .spawn(move || {
                let props = |path: String, json: String| {
                    let _ = qt.queue(move |this| this.properties_changed(QString::from(&path), QString::from(&json)));
                };
                let run = || -> zbus::Result<()> {
                    let conn = Connection::system()?;
                    let stream = signals(&conn, UPOWER)?;
                    let fetch = |path: &str| match get_all(&conn, UPOWER, path, DEVICE_IFACE) {
                        Ok(p) => props(path.to_string(), json_of(&p)),
                        Err(e) => eprintln!("UPower: could not read {path}: {e}"),
                    };
                    props("/".into(), json_of(&get_all(&conn, UPOWER, UPOWER_PATH, UPOWER)?));
                    let upower = Proxy::new(&conn, UPOWER, UPOWER_PATH, UPOWER)?;
                    let display: OwnedObjectPath = upower.call("GetDisplayDevice", &())?;
                    fetch(display.as_str());
                    let path = display.to_string();
                    let _ = qt.queue(move |this| this.display_device(QString::from(&path)));
                    let devices: Vec<OwnedObjectPath> = upower.call("EnumerateDevices", &())?;
                    for d in &devices {
                        fetch(d.as_str());
                    }
                    for msg in stream {
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
                                    fetch(p.as_str());
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
                    Ok(())
                };
                if let Err(e) = run() {
                    eprintln!("UPower: {e}");
                }
            })
            .expect("spawn upower thread");
    }
}

/// power-profiles-daemon: the UPower name first, the legacy net.hadess name as fallback.
const PROFILES: [(&str, &str); 2] = [
    ("org.freedesktop.UPower.PowerProfiles", "/org/freedesktop/UPower/PowerProfiles"),
    ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles"),
];

#[derive(Default)]
pub struct PowerProfilesCoreRust {
    /// Connection and (service, path) once connected.
    target: Arc<Mutex<Option<(Connection, &'static str, &'static str)>>>,
}

impl cxx_qt::Initialize for qobject::PowerProfilesCore {
    fn initialize(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        let target = self.target.clone();
        std::thread::Builder::new()
            .name("power-profiles".into())
            .spawn(move || {
                let emit = |json: String| {
                    let _ = qt.queue(move |this| this.properties_changed(QString::from(&json)));
                };
                let run = || -> zbus::Result<()> {
                    let conn = Connection::system()?;
                    let mut last = None;
                    for (service, path) in PROFILES {
                        let stream = signals(&conn, service)?;
                        match get_all(&conn, service, path, service) {
                            Ok(props) => {
                                *target.lock().unwrap() = Some((conn.clone(), service, path));
                                emit(json_of(&props));
                                for msg in stream {
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
                if let Err(e) = run() {
                    eprintln!("PowerProfiles: {e}");
                }
            })
            .expect("spawn power-profiles thread");
    }
}

impl qobject::PowerProfilesCore {
    fn set_active_profile(&self, profile: &QString) {
        let Some((conn, service, path)) = self.target.lock().unwrap().clone() else {
            eprintln!("PowerProfiles: cannot set the profile, the daemon is not available");
            return;
        };
        let profile = profile.to_string();
        std::thread::spawn(move || {
            let res = Proxy::new(&conn, service, path, PROPS_IFACE)
                .and_then(|p| p.call::<_, _, ()>("Set", &(service, "ActiveProfile", Value::from(profile.as_str()))));
            if let Err(e) = res {
                eprintln!("PowerProfiles: could not set the profile to {profile}: {e}");
            }
        });
    }
}
