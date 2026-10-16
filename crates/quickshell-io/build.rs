use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("Quickshell.Io").version(0, 1).qml_files(["qml/FileView.qml"]))
        .qt_module("Qml")
        .files(["src/datastream.rs", "src/process.rs", "src/fileview.rs", "src/jsonadapter.rs"])
        .build()
        .export();
}
