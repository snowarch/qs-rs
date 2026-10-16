use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell")
            .version(0, 1)
            .qml_files(["qml/Scope.qml", "qml/ShellRoot.qml", "qml/Variants.qml", "qml/Singleton.qml", "qml/ShellScreen.qml", "qml/PanelAnchors.qml", "qml/PanelMargins.qml", "qml/ProxyWindowBase.qml", "qml/PanelWindow.qml", "qml/FloatingWindow.qml", "qml/PopupWindow.qml", "qml/PopupAnchor.qml", "qml/SystemClock.qml", "qml/QsMenuHandle.qml", "qml/QsMenuEntry.qml", "qml/QsMenuOpener.qml"])
            .qml_file(QmlFile::from("qml/Quickshell.qml").singleton(true))
            .qml_file(QmlFile::from("qml/QsMenuButtonType.qml").singleton(true)),
    )
    .qt_module("Quick")
    .cpp_file("include/window.h")
    .cpp_file("include/clipboard.h")
    .cpp_file("include/scriptmodel.h")
    .cpp_file("include/objectmodel.h")
    .cpp_file("include/reload.h")
    .cpp_file("include/lazyloader.h")
    .cpp_file("include/desktopentry.h")
    .files(["src/global.rs", "src/util.rs", "src/colorquantizer.rs", "src/desktopentry.rs"])
    .build()
    .export();
}
