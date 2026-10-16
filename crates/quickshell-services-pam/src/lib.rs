extern crate cxx_qt_lib;
pub mod context;
pub mod helper;

#[cfg(test)]
mod link_init {
    // cxx-qt crates must be initialized in at least one test or the test binary fails to link.
    #[test]
    fn init() {
        cxx_qt::init_crate!(cxx_qt);
        cxx_qt::init_crate!(cxx_qt_lib);
    }
}
