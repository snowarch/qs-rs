use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.Services.SystemTray")
            .version(0, 1)
            .qml_files(["qml/SystemTrayItem.qml"])
            .qml_file(QmlFile::from("qml/SystemTray.qml").singleton(true))
            .qml_file(QmlFile::from("qml/Status.qml").singleton(true))
            .qml_file(QmlFile::from("qml/Category.qml").singleton(true)),
    )
    .files(["src/core.rs"])
    .build()
    .export();
}
