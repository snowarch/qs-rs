//! Qt helpers missing from cxx-qt-lib, implemented in include/qsrs.h.

#[cxx_qt::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("quickshell/include/qsrs.h");

        /// A QVariant holding `nullptr`; QML sees it as `null` (a default QVariant is `undefined`).
        #[namespace = "qsrs"]
        fn null_variant() -> QVariant;
    }
}

pub use ffi::null_variant;
