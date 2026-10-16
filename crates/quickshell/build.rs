use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("Quickshell").version(0, 1))
        .files(["src/global.rs"])
        .build()
        .export();
}
