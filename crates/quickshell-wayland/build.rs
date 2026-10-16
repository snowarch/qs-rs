use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    println!("cargo:rustc-link-lib=LayerShellQtInterface");
    CxxQtBuilder::new_qml_module(QmlModule::new("Quickshell.Wayland")
            .version(0, 1)
            .qml_files(["qml/Toplevel.qml"])
            .qml_file(QmlFile::from("qml/ToplevelManager.qml").singleton(true)),
    )
        .qt_module("Quick")
        .qt_module("Gui")
        .cpp_file("include/layershell.h")
        .files(["src/idle.rs", "src/toplevel.rs"])
        .build()
        .export();
}
