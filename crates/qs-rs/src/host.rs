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
        fn create_app(use_qapplication: bool, desktop_settings_aware: bool) -> UniquePtr<QGuiApplication>;
        fn exec_app(app: Pin<&mut QGuiApplication>) -> i32;
        fn new_engine(import_path: &QString) -> *mut QQmlEngine;
        fn check_file(engine: Pin<&mut QQmlEngine>, path: &QString) -> String;
        unsafe fn create_root(engine: Pin<&mut QQmlEngine>, path: &QString) -> *mut QObject;
    }
}
