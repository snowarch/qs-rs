mod quickshell;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl, QString};

fn main() {
    let path = std::env::args().nth(1).expect("usage: qs-rs <shell.qml>");
    let path = std::fs::canonicalize(path).expect("shell file not found");
    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from_local_file(&QString::from(path.to_str().unwrap())));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
