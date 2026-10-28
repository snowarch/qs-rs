use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};
use std::path::PathBuf;
use std::process::Command;

fn qmake(var: &str) -> String {
    let qmake = std::env::var("QMAKE").unwrap_or_else(|_| "qmake6".into());
    let out = Command::new(&qmake).args(["-query", var]).output().expect("run qmake");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// C bindings from the system wayland-protocols XML (wayland-scanner), used by the C++ headers
/// on Qt's own wayland queue: ext-session-lock-v1 (sessionlock.h), ext-background-effect-v1
/// (backgroundeffect.h).
fn protocols() -> PathBuf {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("protocols");
    std::fs::create_dir_all(&out).unwrap();
    let dir = String::from_utf8(Command::new("pkg-config").args(["--variable=pkgdatadir", "wayland-protocols"]).output().expect("run pkg-config").stdout).unwrap();
    for name in ["ext-session-lock", "ext-background-effect"] {
        let xml = PathBuf::from(dir.trim()).join(format!("staging/{name}/{name}-v1.xml"));
        println!("cargo::rerun-if-changed={}", xml.display());
        for (mode, file) in [("client-header", format!("{name}-v1-client-protocol.h")), ("private-code", format!("{name}-v1-protocol.c"))] {
            let ok = Command::new("wayland-scanner").arg(mode).arg(&xml).arg(out.join(file)).status().expect("run wayland-scanner").success();
            assert!(ok, "wayland-scanner {mode} failed for {name}");
        }
    }
    out
}

fn main() {
    println!("cargo:rustc-link-lib=LayerShellQtInterface");
    println!("cargo:rustc-link-lib=wayland-client");
    println!("cargo:rustc-link-lib=EGL");
    let protocols = protocols();
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
            .qml_files(["qml/Toplevel.qml", "qml/WlSessionLockSurface.qml", "qml/ScreencopyView.qml"])
            .qml_file(QmlFile::from("qml/ToplevelManager.qml").singleton(true)),
    )
    .qt_module("Quick")
    .qt_module("Gui")
    .qt_module("WaylandClient")
    .qt_module("OpenGL")
    .include_dir(&protocols);
    // SAFETY: only adds include paths.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            for dir in &private {
                cc.include(dir);
            }
        })
    };
    builder
        .cpp_file("include/layershell.h")
        .cpp_file("include/sessionlock.h")
        .cpp_file("include/backgroundeffect.h")
        .cpp_file("include/screencopy.h")
        .cpp_file("include/windowcast.h")
        .files(["src/idle.rs", "src/toplevel.rs", "src/screencopy.rs", "src/windowcast.rs", "src/castffi.rs"])
        .build()
        .export();
    // the scanner's code is C: as C++ its hidden visibility lands on a redeclaration and warns. After the C++ lib,
    // so a linker that reads archives in order finds what it uses.
    cc::Build::new()
        .file(protocols.join("ext-session-lock-v1-protocol.c"))
        .file(protocols.join("ext-background-effect-v1-protocol.c"))
        .compile("qs-wayland-protocols");
}
