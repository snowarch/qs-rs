use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};
use std::path::PathBuf;
use std::process::Command;

fn qmake(var: &str) -> String {
    let qmake = std::env::var("QMAKE").unwrap_or_else(|_| "qmake6".into());
    let out = Command::new(&qmake).args(["-query", var]).output().expect("run qmake");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn main() {
    // activation.h reads the last input window and serial: QtWaylandClient's private API, as quickshell-wayland.
    let headers = PathBuf::from(qmake("QT_INSTALL_HEADERS"));
    let version = qmake("QT_VERSION");
    let mut private = vec![];
    for module in ["QtCore", "QtGui", "QtWaylandGlobal", "QtWaylandClient"] {
        private.push(headers.join(module).join(&version));
        private.push(headers.join(module).join(&version).join(module));
    }
    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.Services.SystemTray")
            .version(0, 1)
            .qml_files(["qml/SystemTrayItem.qml"])
            .qml_file(QmlFile::from("qml/SystemTray.qml").singleton(true))
            .qml_file(QmlFile::from("qml/Status.qml").singleton(true))
            .qml_file(QmlFile::from("qml/Category.qml").singleton(true)),
    )
    .qt_module("Gui")
    .qt_module("WaylandClient");
    // SAFETY: only adds include paths.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            for dir in &private {
                cc.include(dir);
            }
        })
    };
    builder.files(["src/core.rs"]).build().export();
}
