use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.Hyprland")
            .version(0, 1)
            .qml_files([
                "qml/HyprlandMonitor.qml",
                "qml/HyprlandWorkspace.qml",
                "qml/HyprlandToplevel.qml",
                "qml/GlobalShortcut.qml",
                "qml/HyprlandFocusGrab.qml",
            ])
            .qml_file(QmlFile::from("qml/Hyprland.qml").singleton(true)),
    )
    .files(["src/ipc.rs"])
    .build()
    .export();
}
