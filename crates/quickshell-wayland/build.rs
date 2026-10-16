use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("Quickshell.Wayland").version(0, 1))
        .qt_module("Quick")
        .cpp_file("include/layershell.h")
        .files([] as [&str; 0])
        .build()
        .export();
}
