#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "Quickshell"]
        #[qml_singleton]
        type QuickshellGlobal = super::QuickshellGlobalRust;

        #[qinvokable]
        fn env(self: &QuickshellGlobal, name: &QString) -> QString;
    }
}

use cxx_qt_lib::QString;

#[derive(Default)]
pub struct QuickshellGlobalRust;

impl qobject::QuickshellGlobal {
    fn env(&self, name: &QString) -> QString {
        std::env::var(name.to_string()).map(|v| QString::from(&v)).unwrap_or_default()
    }
}
