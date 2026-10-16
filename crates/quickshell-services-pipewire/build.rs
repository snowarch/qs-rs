use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.Services.Pipewire")
            .version(0, 1)
            .qml_files([
                "qml/PwNode.qml",
                "qml/PwNodeAudio.qml",
                "qml/PwLink.qml",
                "qml/PwLinkGroup.qml",
                "qml/PwObjectTracker.qml",
                "qml/PwNodeLinkTracker.qml",
            ])
            .qml_file(QmlFile::from("qml/Pipewire.qml").singleton(true))
            .qml_file(QmlFile::from("qml/PwNodeType.qml").singleton(true))
            .qml_file(QmlFile::from("qml/PwLinkState.qml").singleton(true))
            .qml_file(QmlFile::from("qml/PwAudioChannel.qml").singleton(true)),
    )
    .files(["src/core.rs"])
    .build()
    .export();
}
