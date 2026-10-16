#[cxx_qt::bridge]
pub mod ffi {
    #[namespace = "qswl"]
    unsafe extern "C++" {
        include!("qs_wayland/include/display.h");
        /// Qt's wl_display, or 0 when Qt does not run on Wayland.
        fn qt_wl_display() -> usize;
    }
}
