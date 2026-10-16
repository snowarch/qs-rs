use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn qmake(var: &str) -> String {
    let qmake = std::env::var("QMAKE").unwrap_or_else(|_| "qmake6".into());
    let out = std::process::Command::new(&qmake).args(["-query", var]).output().expect("run qmake");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn main() {
    // QRhi headers live in QtGui's versioned (semi-public) include dir.
    let gui = std::path::PathBuf::from(qmake("QT_INSTALL_HEADERS")).join("QtGui").join(qmake("QT_VERSION"));
    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell")
            .version(0, 1)
            .qml_files(["qml/Scope.qml", "qml/ShellRoot.qml", "qml/Variants.qml", "qml/Singleton.qml", "qml/ShellScreen.qml", "qml/PanelAnchors.qml", "qml/PanelMargins.qml", "qml/ProxyWindowBase.qml", "qml/PanelWindow.qml", "qml/FloatingWindow.qml", "qml/PopupWindow.qml", "qml/PopupAnchor.qml", "qml/SystemClock.qml", "qml/QsMenuHandle.qml", "qml/QsMenuEntry.qml", "qml/QsMenuOpener.qml"])
            .qml_file(QmlFile::from("qml/Quickshell.qml").singleton(true))
            .qml_file(QmlFile::from("qml/QsMenuButtonType.qml").singleton(true)),
    )
    .qt_module("Quick");
    // SAFETY: only adds an include path.
    let builder = unsafe {
        builder.cc_builder(move |cc| {
            cc.include(&gui);
            cc.include(gui.join("QtGui"));
        })
    };
    builder
    .cpp_file("include/window.h")
    .cpp_file("include/clipboard.h")
    .cpp_file("include/scriptmodel.h")
    .cpp_file("include/objectmodel.h")
    .cpp_file("include/reload.h")
    .cpp_file("include/lazyloader.h")
    .cpp_file("include/desktopentry.h")
    .files(["src/global.rs", "src/util.rs", "src/colorquantizer.rs", "src/desktopentry.rs", "src/log.rs"])
    .build()
    .export();
}
