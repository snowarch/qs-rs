//! FFI to include/host.h.

#[cxx::bridge(namespace = "qsrs")]
pub mod ffi {
    unsafe extern "C++" {
        include!("qs-rs/include/host.h");
        #[namespace = ""]
        type QGuiApplication = cxx_qt_lib::QGuiApplication;
        #[namespace = ""]
        type QQmlEngine = cxx_qt_lib::QQmlEngine;
        #[namespace = ""]
        type QString = cxx_qt_lib::QString;
        #[namespace = ""]
        type QObject = cxx_qt::QObject;

        fn install_message_handler();
        fn install_crash_handler();
        fn install_quit_signals();
        fn start_qml_debug_server(port: i32, block: bool) -> bool;
        fn qt_build_version() -> String;
        fn qt_plugins_path() -> String;
        fn set_plugin_path(path: &QString);
        fn setup_icons(theme: &QString);
        fn init_webengine() -> bool;
        fn create_app(use_qapplication: bool, desktop_settings_aware: bool) -> UniquePtr<QGuiApplication>;
        fn configure_app(app_id: &QString, native_text_rendering: bool);
        fn exec_app(app: Pin<&mut QGuiApplication>) -> i32;
        fn new_engine(import_path: &QString) -> *mut QQmlEngine;
        fn check_file(engine: Pin<&mut QQmlEngine>, path: &QString) -> String;
        unsafe fn create_root(engine: Pin<&mut QQmlEngine>, path: &QString) -> *mut QObject;
        unsafe fn install_reload(engine: *mut QQmlEngine, root: *mut QObject, import_path: &QString, entry: &QString);
        fn request_reload(hard: bool);
    }

    extern "Rust" {
        fn rescan_vfs() -> String;
    }
}

/// What a reload scans again: the shell's root file and the VFS it mirrors into.
pub static SHELL: std::sync::OnceLock<(std::path::PathBuf, std::path::PathBuf)> = std::sync::OnceLock::new();

/// Called by the host on reload (upstream reloadGraph's scan): re-scans the shell and rewrites the
/// VFS. Returns the error, empty when the new tree is in place.
fn rescan_vfs() -> String {
    let Some((root_file, vfs)) = SHELL.get() else { return "no shell to reload".into() };
    match crate::scan::Scan::new(root_file).and_then(|scan| scan.write_vfs(vfs).map(|()| scan)) {
        Ok(scan) => {
            crate::watchfiles::track(scan.hashes);
            String::new()
        }
        Err(e) => format!("scan of {} failed: {e}", root_file.display()),
    }
}
