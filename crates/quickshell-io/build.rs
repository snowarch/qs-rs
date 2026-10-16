use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("Quickshell.Io").version(0, 1))
        .files(["src/datastream.rs", "src/process.rs"])
        .build()
        .export();
}
