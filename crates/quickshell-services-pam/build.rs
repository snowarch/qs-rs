use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.Services.Pam")
            .version(0, 1)
            .qml_files(["qml/PamContext.qml"])
            .qml_file(QmlFile::from("qml/PamResult.qml").singleton(true))
            .qml_file(QmlFile::from("qml/PamError.qml").singleton(true)),
    )
    .files(["src/context.rs"])
    .build()
    .export();
}
