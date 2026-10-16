fn main() {
    cxx_qt_build::CxxQtBuilder::new().qt_module("Widgets").qt_module("Quick").files(["src/host.rs"]).build();
}
