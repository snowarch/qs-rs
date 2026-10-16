extern crate quickshell;
extern crate quickshell_io;

mod host;
mod launch;
mod scan;

use cxx_qt_lib::QString;
use quickshell::common::{ShellInfo, shell_dir};
use std::path::PathBuf;

fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("qs-rs")
}

fn qstr(p: &std::path::Path) -> QString {
    QString::from(&*p.to_string_lossy())
}

fn main() {
    quickshell::common::capture_initial_env();
    cxx_qt::init_crate!(quickshell);
    cxx_qt::init_crate!(quickshell_io);

    let arg = std::env::args().nth(1).expect("usage: qs-rs <shell.qml>");
    let root_file = std::fs::canonicalize(&arg).expect("shell file not found");
    let text = std::fs::read_to_string(&root_file).expect("could not read shell file");
    let pragmas = match launch::parse(&text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let path_id = quickshell::util::md5_hex(&qstr(&root_file)).to_string();
    let shell_id = pragmas.shell_id.clone().unwrap_or(path_id);
    let info = ShellInfo {
        shell_dir: root_file.parent().unwrap().to_path_buf(),
        data_dir: shell_dir("XDG_DATA_HOME", ".local/share", &shell_id, pragmas.data_dir.as_deref()),
        state_dir: shell_dir("XDG_STATE_HOME", ".local/state", &shell_id, pragmas.state_dir.as_deref()),
        cache_dir: shell_dir("XDG_CACHE_HOME", ".cache", &shell_id, pragmas.cache_dir.as_deref()),
        shell_id,
    };
    for d in [&info.data_dir, &info.state_dir, &info.cache_dir] {
        let _ = std::fs::create_dir_all(d);
    }
    quickshell::common::set_shell(info);
    launch::apply_env(&pragmas);

    let scan = scan::Scan::new(&root_file).expect("scan failed");
    let vfs_root = runtime_dir().join(format!("vfs-{}", std::process::id()));
    let vfs = vfs_root.join("qs");
    scan.write_vfs(&vfs).expect("vfs write failed");
    let entry = vfs.join(root_file.strip_prefix(&scan.root).unwrap());

    let mut app = host::ffi::create_app(pragmas.use_qapplication, pragmas.desktop_settings_aware);
    let engine = host::ffi::new_engine(&qstr(&vfs_root));
    let root = unsafe { host::ffi::create_root(std::pin::Pin::new_unchecked(&mut *engine), &qstr(&entry)) };
    let code = if root.is_null() { 1 } else { host::ffi::exec_app(app.pin_mut()) };
    let _ = std::fs::remove_dir_all(&vfs_root);
    std::process::exit(code);
}
