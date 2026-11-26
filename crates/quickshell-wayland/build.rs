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
/// (backgroundeffect.h), and Rin's rin-window-view-v1 (windowview.h, its XML kept in protocols/).
fn protocols() -> PathBuf {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("protocols");
    std::fs::create_dir_all(&out).unwrap();
    let dir = String::from_utf8(Command::new("pkg-config").args(["--variable=pkgdatadir", "wayland-protocols"]).output().expect("run pkg-config").stdout).unwrap();
    let ours = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("protocols");
    for name in ["ext-session-lock", "ext-background-effect", "rin-window-view"] {
        let xml = if name.starts_with("rin-") { ours.join(format!("{name}-v1.xml")) } else { PathBuf::from(dir.trim()).join(format!("staging/{name}/{name}-v1.xml")) };
        println!("cargo::rerun-if-changed={}", xml.display());
        for (mode, file) in [("client-header", format!("{name}-v1-client-protocol.h")), ("private-code", format!("{name}-v1-protocol.c"))] {
            let ok = Command::new("wayland-scanner").arg(mode).arg(&xml).arg(out.join(file)).status().expect("run wayland-scanner").success();
            assert!(ok, "wayland-scanner {mode} failed for {name}");
        }
    }
    out
}

/// WindowView's hole shaders, baked with Qt's qsb like quickshell-widgets' and listed in a qrc under the module.
fn shaders() -> PathBuf {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let bins = qmake("QT_HOST_BINS");
    let qsb = [format!("{bins}/qsb"), "qsb".into()].into_iter().find(|p| Command::new(p).arg("--version").output().is_ok()).expect("qsb (Qt Shader Tools) not found");
    let mut entries = String::new();
    for name in ["windowhole.vert", "windowhole.frag"] {
        let src = PathBuf::from("shaders").join(name).canonicalize().unwrap();
        println!("cargo::rerun-if-changed={}", src.display());
        let target = out.join(format!("{name}.qsb"));
        let status = Command::new(&qsb).args(["--glsl", "100es,120,150", "--batchable", "-O", "-o"]).arg(&target).arg(&src).status().expect("run qsb");
        assert!(status.success(), "qsb failed for {name}");
        entries.push_str(&format!("<file alias=\"shaders/{name}.qsb\">{}</file>", target.display()));
    }
    let qrc = out.join("wayland.qrc");
    std::fs::write(&qrc, format!("<RCC><qresource prefix=\"/qt/qml/Quickshell/Wayland\">{entries}</qresource></RCC>")).unwrap();
    qrc
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
    for module in ["QtCore", "QtGui", "QtQml", "QtQuick", "QtWaylandGlobal", "QtWaylandClient"] {
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
    .include_dir(&protocols)
    .qrc(shaders());
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
        .cpp_file("include/windowview.h")
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
        .file(protocols.join("rin-window-view-v1-protocol.c"))
        .compile("qs-wayland-protocols");
}
