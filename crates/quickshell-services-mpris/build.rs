use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.Services.Mpris")
            .version(0, 1)
            .qml_files(["qml/MprisPlayer.qml"])
            .qml_file(QmlFile::from("qml/Mpris.qml").singleton(true))
            .qml_file(QmlFile::from("qml/MprisPlaybackState.qml").singleton(true))
            .qml_file(QmlFile::from("qml/MprisLoopState.qml").singleton(true)),
    )
    .files(["src/core.rs"])
    .build()
    .export();
}
