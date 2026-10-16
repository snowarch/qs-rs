use cxx_qt_build::CxxQtBuilder;

fn main() {
    CxxQtBuilder::new().qt_module("Gui").files(["src/qt.rs"]).build().export();
}
