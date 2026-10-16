use cxx_qt_build::{CxxQtBuilder, QmlModule};
use std::path::PathBuf;
use std::process::Command;

/// Bakes the clipping shader with Qt's qsb (both variants, like upstream's qt6_add_shaders) and
/// lists them in a qrc next to the module's QML files.
fn shaders() -> PathBuf {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let src = PathBuf::from("shaders/cliprect.frag").canonicalize().unwrap();
    println!("cargo::rerun-if-changed={}", src.display());
    let qsb = ["/usr/lib/qt6/bin/qsb", "qsb"].into_iter().find(|p| Command::new(p).arg("--version").output().is_ok()).expect("qsb (Qt Shader Tools) not found");
    let mut entries = String::new();
    for (name, define) in [("cliprect.frag.qsb", None), ("cliprect-ub.frag.qsb", Some("CONTENT_UNDER_BORDER"))] {
        let target = out.join(name);
        let mut cmd = Command::new(qsb);
        cmd.args(["--glsl", "100es,120,150", "--batchable", "-O", "-o"]).arg(&target);
        if let Some(d) = define {
            cmd.arg(format!("-D{d}"));
        }
        let status = cmd.arg(&src).status().expect("run qsb");
        assert!(status.success(), "qsb failed for {name}");
        entries.push_str(&format!("<file alias=\"shaders/{name}\">{}</file>", target.display()));
    }
    let qrc = out.join("widgets.qrc");
    std::fs::write(&qrc, format!("<RCC><qresource prefix=\"/qt/qml/Quickshell/Widgets\">{entries}</qresource></RCC>")).unwrap();
    qrc
}

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.Widgets").version(0, 1).qml_files(["qml/IconImage.qml", "qml/ClippingRectangle.qml"]),
    )
    .qt_module("Quick")
    .cpp_file("include/cliprect.h")
    .qrc(shaders())
    .files([] as [&str; 0])
    .build()
    .export();
}
