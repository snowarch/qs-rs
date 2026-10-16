#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "Quickshell"]
        #[qml_singleton]
        type QuickshellGlobal = super::QuickshellGlobalRust;

        #[qinvokable]
        fn env(self: &QuickshellGlobal, name: &QString) -> QVariant;
    }
}

use cxx_qt_lib::{QString, QVariant};

#[derive(Default)]
pub struct QuickshellGlobalRust;

impl qobject::QuickshellGlobal {
    fn env(&self, name: &QString) -> QVariant {
        match std::env::var(name.to_string()) {
            Ok(v) => QVariant::from(&QString::from(&v)),
            Err(_) => crate::util::null_variant(),
        }
    }
}
