//! Quickshell.Wayland: layer shell attached properties (include/layershell.h), idle monitor and
//! foreign toplevels.
extern crate cxx_qt_lib;
extern crate quickshell;
pub mod idle;
pub mod toplevel;

#[cfg(test)]
mod link_init {
    // cxx-qt crates must be initialized in at least one test or the test binary fails to link.
    #[test]
    fn init() {
        cxx_qt::init_crate!(cxx_qt);
        cxx_qt::init_crate!(cxx_qt_lib);
        cxx_qt::init_crate!(quickshell);
    }
}
