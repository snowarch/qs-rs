use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.Services.Notifications")
            .version(0, 1)
            .qml_files(["qml/NotificationServer.qml", "qml/Notification.qml", "qml/NotificationAction.qml"])
            .qml_file(QmlFile::from("qml/NotificationUrgency.qml").singleton(true))
            .qml_file(QmlFile::from("qml/NotificationCloseReason.qml").singleton(true)),
    )
    .files(["src/core.rs"])
    .build()
    .export();
}
