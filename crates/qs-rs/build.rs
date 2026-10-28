fn main() {
    cxx_qt_build::CxxQtBuilder::new().qt_module("Widgets").qt_module("Quick").qt_module("DBus").files(["src/host.rs"]).build();
    // The system jemalloc as the allocator for the whole process (Qt, Mesa and Rust), like
    // upstream: glibc's per-thread arenas keep what the QML engine frees (~100 MB on inir).
    // The binary's own libs come first on the link line, so it lands in DT_NEEDED ahead of libc
    // and its malloc wins symbol lookup; main.rs calls mallctl so it is not dropped as unneeded.
    // The binary's `malloc_conf` default must be visible to it.
    println!("cargo:rustc-link-lib=dylib=jemalloc");
    // `--version` build details: git revision, compiler and flags (upstream debuginfo).
    let git = |args: &[&str]| {
        std::process::Command::new("git").args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    println!("cargo:rustc-env=QSRS_REVISION={}", git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into()));
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={dir}/HEAD");
        if let Some(r) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={dir}/{r}");
        }
    }
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = std::process::Command::new(&rustc).arg("--version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    println!("cargo:rustc-env=QSRS_RUSTC={}", version.unwrap_or(rustc));
    println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
    println!("cargo:rustc-env=QSRS_RUSTFLAGS={}", std::env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default().replace('\x1f', " "));
    println!("cargo:rustc-link-arg-bins=-Wl,--export-dynamic-symbol=malloc_conf");
}
