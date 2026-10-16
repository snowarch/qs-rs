use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.DBusMenu")
            .version(0, 1)
            .qml_files(["qml/DBusMenuHandle.qml", "qml/DBusMenuItem.qml"]),
    )
    .files(["src/client.rs"])
    .build()
    .export();
}
