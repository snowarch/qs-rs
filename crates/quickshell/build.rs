use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell")
            .version(0, 1)
            .qml_files(["qml/Scope.qml", "qml/ShellRoot.qml", "qml/LazyLoader.qml", "qml/Variants.qml"]),
    )
    .qt_module("Quick")
    .files(["src/global.rs", "src/util.rs"])
    .build()
    .export();
}
