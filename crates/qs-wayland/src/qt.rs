#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    #[namespace = "qswl"]
    unsafe extern "C++" {
        include!("qs_wayland/include/display.h");
        /// Qt's wl_display, or 0 when Qt does not run on Wayland.
        fn qt_wl_display() -> usize;
        /// Name of the QScreen whose wl_output proxy is `output`, or empty.
        fn screen_name_for_output(output: usize) -> QString;
        /// Qt's wl_output proxy for the screen named `name`, or 0.
        fn output_for_screen_name(name: &QString) -> usize;
    }
}
