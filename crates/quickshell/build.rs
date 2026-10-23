use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn qmake(var: &str) -> String {
    let qmake = std::env::var("QMAKE").unwrap_or_else(|_| "qmake6".into());
    let out = std::process::Command::new(&qmake).args(["-query", var]).output().expect("run qmake");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn main() {
    // QRhi headers live in QtGui's versioned (semi-public) include dir, QQuickWindowPrivate
    // (updatesEnabled) in QtQuick's, which pulls in the other modules' private headers.
    let headers = std::path::PathBuf::from(qmake("QT_INSTALL_HEADERS"));
    let version = qmake("QT_VERSION");
    let private: Vec<_> = ["QtCore", "QtGui", "QtQml", "QtQmlMeta", "QtQmlModels", "QtQuick"]
        .iter()
        .flat_map(|m| { let d = headers.join(m).join(&version); [d.clone(), d.join(m)] })
        .filter(|d| d.is_dir())
        .collect();
    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell")
            .version(0, 1)
            .qml_files(["qml/Scope.qml", "qml/ShellRoot.qml", "qml/Variants.qml", "qml/Singleton.qml", "qml/ShellScreen.qml", "qml/PanelAnchors.qml", "qml/PanelMargins.qml", "qml/ProxyWindowBase.qml", "qml/PanelWindow.qml", "qml/FloatingWindow.qml", "qml/PopupWindow.qml", "qml/SystemClock.qml", "qml/QsMenuHandle.qml", "qml/QsMenuEntry.qml", "qml/QsMenuOpener.qml"])
            .qml_file(QmlFile::from("qml/Quickshell.qml").singleton(true))
            .qml_file(QmlFile::from("qml/QsMenuButtonType.qml").singleton(true)),
    )
    .qt_module("Quick");
    // SAFETY: only adds an include path.
    let builder = unsafe {
        builder.cc_builder(move |cc| {
            for d in &private {
                cc.include(d);
            }
            cc.file("src/cpp/windowprivate.cpp");
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
    .cpp_file("include/springvalue.h")
    .cpp_file("include/fieldregion.h")
    .files(["src/global.rs", "src/util.rs", "src/colorquantizer.rs", "src/desktopentry.rs", "src/errorlog.rs", "src/log.rs"])
    .build()
    .export();
}
