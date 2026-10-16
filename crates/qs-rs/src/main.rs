extern crate quickshell;
extern crate quickshell_io;

mod scan;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};
use std::path::PathBuf;

fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("qs-rs")
}

fn main() {
    cxx_qt::init_crate!(quickshell);
    cxx_qt::init_crate!(quickshell_io);
    let arg = std::env::args().nth(1).expect("usage: qs-rs <shell.qml>");
    let root_file = std::fs::canonicalize(&arg).expect("shell file not found");
    let scan = scan::Scan::new(&root_file).expect("scan failed");
    let vfs_root = runtime_dir().join(format!("vfs-{}", std::process::id()));
    let vfs = vfs_root.join("qs");
    scan.write_vfs(&vfs).expect("vfs write failed");
    let entry = vfs.join(root_file.strip_prefix(&scan.root).unwrap());

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        engine.as_mut().add_import_path(&QString::from(vfs_root.to_str().unwrap()));
        engine.load(&QUrl::from_local_file(&QString::from(entry.to_str().unwrap())));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
    let _ = std::fs::remove_dir_all(&vfs_root);
}
