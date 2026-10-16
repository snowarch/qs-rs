use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.Services.Polkit").version(0, 1).qml_files(["qml/PolkitAgent.qml", "qml/AuthFlow.qml"]),
    )
    .files(["src/agent.rs"])
    .build()
    .export();
}
