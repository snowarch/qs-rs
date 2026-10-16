//! BlueZ on the system bus: the object manager stream feeds QML, and property writes and
//! method calls run in order on one worker thread so the Qt thread never waits on the bus.

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
        #[qml_element = "BluezCore"]
        #[qml_singleton]
        type BluezCore = super::BluezCoreRust;

        #[qinvokable]
        #[cxx_name = "setBool"]
        fn set_bool(self: &BluezCore, path: &QString, iface: &QString, name: &QString, value: bool);
        #[qinvokable]
        #[cxx_name = "setUInt"]
        fn set_uint(self: &BluezCore, path: &QString, iface: &QString, name: &QString, value: u32);
        #[qinvokable]
        #[cxx_name = "setString"]
        fn set_string(self: &BluezCore, path: &QString, iface: &QString, name: &QString, value: &QString);
        /// Calls `method` with no arguments, or with `objectArg` as an object path when not empty.
        /// Answers through `callFinished`.
        #[qinvokable]
        fn call(self: &BluezCore, path: &QString, iface: &QString, method: &QString, object_arg: &QString);

        #[qsignal]
        #[cxx_name = "interfacesAdded"]
        fn interfaces_added(self: Pin<&mut BluezCore>, path: QString, json: QString);
        #[qsignal]
        #[cxx_name = "interfacesRemoved"]
        fn interfaces_removed(self: Pin<&mut BluezCore>, path: QString, interfaces: QStringList);
        #[qsignal]
        #[cxx_name = "propertiesChanged"]
        fn properties_changed(self: Pin<&mut BluezCore>, path: QString, iface: QString, json: QString);
        #[qsignal]
        #[cxx_name = "callFinished"]
        fn call_finished(self: Pin<&mut BluezCore>, path: QString, method: QString, error: QString);
    }

    impl cxx_qt::Initialize for BluezCore {}
    impl cxx_qt::Threading for BluezCore {}
}

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::{QString, QStringList};
use qs_dbus::object_manager::{self, Event};
use qs_dbus::zbus::blocking::Connection;
use qs_dbus::SetValue;
use std::sync::{Arc, OnceLock};

const BLUEZ: &str = "org.bluez";

#[derive(Default)]
pub struct BluezCoreRust {
    worker: Arc<OnceLock<qs_dbus::Worker>>,
}

impl cxx_qt::Initialize for qobject::BluezCore {
    fn initialize(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        let slot = self.worker.clone();
        std::thread::Builder::new()
            .name("bluez".into())
            .spawn(move || {
                let conn = match Connection::system() {
                    Ok(c) => {
                        slot.get_or_init(|| qs_dbus::Worker::new(c.clone(), "bluez-calls"));
                        c
                    }
                    Err(e) => return eprintln!("Bluetooth: no system bus: {e}"),
                };
                let res = object_manager::run(&conn, BLUEZ, "/", |event| {
                    let _ = match event {
                        Event::InterfacesAdded(path, json) => {
                            qt.queue(move |this| this.interfaces_added(QString::from(&path), QString::from(&json)))
                        }
                        Event::InterfacesRemoved(path, ifaces) => qt.queue(move |this| {
                            let mut list = QStringList::default();
                            for i in &ifaces {
                                list.append(QString::from(i));
                            }
                            this.interfaces_removed(QString::from(&path), list)
                        }),
                        Event::PropertiesChanged(path, iface, json) => qt.queue(move |this| {
                            this.properties_changed(QString::from(&path), QString::from(&iface), QString::from(&json))
                        }),
                    };
                });
                if let Err(e) = res {
                    eprintln!("Bluetooth: {e}");
                }
            })
            .expect("spawn bluez thread");
    }
}

impl qobject::BluezCore {
    fn set(&self, path: &QString, iface: &QString, name: &QString, value: SetValue) {
        let Some(worker) = self.worker.get() else { return };
        let (path, iface, name) = (path.to_string(), iface.to_string(), name.to_string());
        worker.run(format!("Bluetooth: setting {name} on {path}"), move |conn| qs_dbus::set_property(conn, BLUEZ, &path, &iface, &name, value));
    }

    fn set_bool(&self, path: &QString, iface: &QString, name: &QString, value: bool) {
        self.set(path, iface, name, SetValue::Bool(value));
    }

    fn set_uint(&self, path: &QString, iface: &QString, name: &QString, value: u32) {
        self.set(path, iface, name, SetValue::U32(value));
    }

    fn set_string(&self, path: &QString, iface: &QString, name: &QString, value: &QString) {
        self.set(path, iface, name, SetValue::Str(value.to_string()));
    }

    fn call(&self, path: &QString, iface: &QString, method: &QString, object_arg: &QString) {
        let Some(worker) = self.worker.get() else { return };
        let (path, iface, method, arg) = (path.to_string(), iface.to_string(), method.to_string(), object_arg.to_string());
        let qt = self.qt_thread();
        worker.run(format!("Bluetooth: {method} on {path}"), move |conn| {
            let res = qs_dbus::call(conn, BLUEZ, &path, &iface, &method, (!arg.is_empty()).then_some(arg.as_str()));
            let error = res.as_ref().err().map(|e| e.to_string()).unwrap_or_default();
            let _ = qt.queue(move |this| this.call_finished(QString::from(&path), QString::from(&method), QString::from(&error)));
            res
        });
    }
}
