use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};
use std::path::PathBuf;
use std::process::Command;

fn qmake(var: &str) -> String {
    let qmake = std::env::var("QMAKE").unwrap_or_else(|_| "qmake6".into());
    let out = Command::new(&qmake).args(["-query", var]).output().expect("run qmake");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// C bindings for ext-session-lock-v1 from the system wayland-protocols XML (wayland-scanner),
/// used by include/sessionlock.h on Qt's own wayland queue.
fn session_lock_protocol() -> PathBuf {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("protocols");
    std::fs::create_dir_all(&out).unwrap();
    let dir = String::from_utf8(Command::new("pkg-config").args(["--variable=pkgdatadir", "wayland-protocols"]).output().expect("run pkg-config").stdout).unwrap();
    let xml = PathBuf::from(dir.trim()).join("staging/ext-session-lock/ext-session-lock-v1.xml");
    println!("cargo::rerun-if-changed={}", xml.display());
    for (mode, file) in [("client-header", "ext-session-lock-v1-client-protocol.h"), ("private-code", "ext-session-lock-v1-protocol.c")] {
        let ok = Command::new("wayland-scanner").arg(mode).arg(&xml).arg(out.join(file)).status().expect("run wayland-scanner").success();
        assert!(ok, "wayland-scanner {mode} failed");
    }
    // Built as C++: the header's extern "C" declarations keep the interfaces' external C linkage.
    std::fs::write(out.join("ext-session-lock-v1-protocol.cpp"), "#include \"ext-session-lock-v1-client-protocol.h\"\n#include \"ext-session-lock-v1-protocol.c\"\n").unwrap();
    out
}

fn main() {
    println!("cargo:rustc-link-lib=LayerShellQtInterface");
    println!("cargo:rustc-link-lib=wayland-client");
    let protocols = session_lock_protocol();
    // QtWaylandClient's shell integration API is private (as in upstream and LayerShellQt).
    let headers = PathBuf::from(qmake("QT_INSTALL_HEADERS"));
    let version = qmake("QT_VERSION");
    let mut private = vec![];
    for module in ["QtCore", "QtGui", "QtWaylandGlobal", "QtWaylandClient"] {
        private.push(headers.join(module).join(&version));
        private.push(headers.join(module).join(&version).join(module));
    }
    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.Wayland")
            .version(0, 1)
            .qml_files(["qml/Toplevel.qml", "qml/WlSessionLockSurface.qml"])
            .qml_file(QmlFile::from("qml/ToplevelManager.qml").singleton(true)),
    )
    .qt_module("Quick")
    .qt_module("Gui")
    .qt_module("WaylandClient")
    .include_dir(&protocols);
    // SAFETY: only adds include paths and one C source file.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            for dir in &private {
                cc.include(dir);
            }
            cc.file(protocols.join("ext-session-lock-v1-protocol.cpp"));
        })
    };
    builder
        .cpp_file("include/layershell.h")
        .cpp_file("include/sessionlock.h")
        .files(["src/idle.rs", "src/toplevel.rs"])
        .build()
        .export();
}
