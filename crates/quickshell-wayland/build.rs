use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    println!("cargo:rustc-link-lib=LayerShellQtInterface");
    CxxQtBuilder::new_qml_module(QmlModule::new("Quickshell.Wayland").version(0, 1))
        .qt_module("Quick")
        .qt_module("Gui")
        .cpp_file("include/layershell.h")
        .files(["src/idle.rs"])
        .build()
        .export();
}
