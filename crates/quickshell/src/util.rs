//! Qt helpers missing from cxx-qt-lib, implemented in include/qsrs.h.

#[cxx_qt::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qqmlengine.h");
        type QQmlEngine = cxx_qt_lib::QQmlEngine;
        include!("quickshell/include/qsrs.h");

        /// A QVariant holding `nullptr`; QML sees it as `null` (a default QVariant is `undefined`).
        #[namespace = "qsrs"]
        fn null_variant() -> QVariant;
        #[namespace = "qsrs"]
        fn qt_version() -> QString;
        #[namespace = "qsrs"]
        fn load_pixels(path: &QString, x: i32, y: i32, w: i32, h: i32, rescale: f64) -> Vec<u32>;
        #[namespace = "qsrs"]
        fn md5_hex(s: &QString) -> QString;
        #[namespace = "qsrs"]
        fn has_theme_icon(name: &QString) -> bool;
        /// Registers `image://icon/...` on the engine.
        #[namespace = "qsrs"]
        fn install_image_providers(engine: Pin<&mut QQmlEngine>);
    }
}

pub use ffi::{has_theme_icon, install_image_providers, load_pixels, md5_hex, null_variant, qt_version};
