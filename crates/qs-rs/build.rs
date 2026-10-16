fn main() {
    cxx_qt_build::CxxQtBuilder::new().qt_module("Widgets").qt_module("Quick").files(["src/host.rs"]).build();
    // The system jemalloc as the allocator for the whole process (Qt, Mesa and Rust), like
    // upstream: glibc's per-thread arenas keep what the QML engine frees (~100 MB on inir).
    // The binary's own libs come first on the link line, so it lands in DT_NEEDED ahead of libc
    // and its malloc wins symbol lookup; main.rs calls mallctl so it is not dropped as unneeded.
    // The binary's `malloc_conf` default must be visible to it.
    println!("cargo:rustc-link-lib=dylib=jemalloc");
    println!("cargo:rustc-link-arg-bins=-Wl,--export-dynamic-symbol=malloc_conf");
}
