//! Quickshell.Widgets: IconImage and ClippingRectangle (QML over a baked shader, plus the
//! clippingRectangleBorder value type in include/cliprect.h).
extern crate cxx_qt_lib;

#[cfg(test)]
mod link_init {
    // cxx-qt crates must be initialized in at least one test or the test binary fails to link.
    #[test]
    fn init() {
        cxx_qt::init_crate!(cxx_qt);
        cxx_qt::init_crate!(cxx_qt_lib);
    }
}
