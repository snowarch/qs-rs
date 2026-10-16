use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell")
            .version(0, 1)
            .qml_files(["qml/Scope.qml", "qml/ShellRoot.qml", "qml/LazyLoader.qml", "qml/Variants.qml", "qml/Singleton.qml", "qml/ShellScreen.qml"])
            .qml_file(QmlFile::from("qml/Quickshell.qml").singleton(true)),
    )
    .qt_module("Quick")
    .cpp_file("include/window.h")
    .cpp_file("include/clipboard.h")
    .files(["src/global.rs", "src/util.rs"])
    .build()
    .export();
}
