extern crate quickshell;
extern crate quickshell_io;
extern crate quickshell_wayland;
extern crate quickshell_hyprland;
extern crate quickshell_services_pipewire;
extern crate quickshell_services_upower;
extern crate quickshell_bluetooth;
extern crate quickshell_services_mpris;
extern crate quickshell_services_notifications;

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

const USAGE: &str = "usage: qs-rs [-p <path> | -c <name>] [ipc (call <target> <function> [args...] | show)]";

/// Resolves the shell file like quickshell: `-p` file or directory, `-c` a named config under
/// `$XDG_CONFIG_HOME/quickshell/`, or a bare path argument.
fn resolve_shell(path: Option<String>, name: Option<String>) -> PathBuf {
    let candidate = match (path, name) {
        (Some(p), _) => PathBuf::from(p),
        (None, Some(n)) => {
            let base = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
            base.join("quickshell").join(n)
        }
        (None, None) => {
            eprintln!("{USAGE}");
            std::process::exit(1);
        }
    };
    let file = if candidate.is_dir() { candidate.join("shell.qml") } else { candidate };
    match std::fs::canonicalize(&file) {
        Ok(f) => f,
        Err(_) => {
            eprintln!("Could not find shell file {}", file.display());
            std::process::exit(1);
        }
    }
}

fn shell_id_for(root_file: &std::path::Path, pragmas: &launch::Pragmas) -> String {
    pragmas.shell_id.clone().unwrap_or_else(|| quickshell::util::md5_hex(&qstr(root_file)).to_string())
}

fn ipc_main(root_file: &std::path::Path, pragmas: &launch::Pragmas, args: &[String]) -> ! {
    let fields: Vec<String> = match args.first().map(String::as_str) {
        Some("call") if args.len() >= 3 => std::iter::once("call".to_string()).chain(args[1..].iter().cloned()).collect(),
        Some("show") => vec!["show".into()],
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(1);
        }
    };
    match quickshell_io::ipc::request(&shell_id_for(root_file, pragmas), &fields) {
        Ok((0, text)) => {
            if !text.is_empty() {
                println!("{}", text.trim_end_matches('\n'));
            }
            std::process::exit(0)
        }
        Ok((_, text)) => {
            eprintln!("{text}");
            std::process::exit(1)
        }
        Err(e) => {
            eprintln!("No running instance for {}: {e}", root_file.display());
            std::process::exit(1)
        }
    }
}

fn main() {
    quickshell::common::capture_initial_env();
    cxx_qt::init_crate!(quickshell);
    cxx_qt::init_crate!(quickshell_io);
    cxx_qt::init_crate!(quickshell_wayland);
    cxx_qt::init_crate!(quickshell_hyprland);
    cxx_qt::init_crate!(quickshell_services_pipewire);
    cxx_qt::init_crate!(quickshell_services_upower);
    cxx_qt::init_crate!(quickshell_bluetooth);
    cxx_qt::init_crate!(quickshell_services_mpris);
    cxx_qt::init_crate!(quickshell_services_notifications);

    let mut args = std::env::args().skip(1).peekable();
    let (mut path, mut name, mut rest) = (None, None, Vec::new());
    while let Some(a) = args.next() {
        match a.as_str() {
            "-p" | "--path" => path = args.next(),
            "-c" | "--config" => name = args.next(),
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            _ if rest.is_empty() && path.is_none() && a.ends_with(".qml") => path = Some(a),
            _ => rest.push(a),
        }
    }
    let root_file = resolve_shell(path, name);
    let text = std::fs::read_to_string(&root_file).expect("could not read shell file");
    let pragmas = match launch::parse(&text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if rest.first().is_some_and(|a| a == "ipc") {
        ipc_main(&root_file, &pragmas, &rest[1..]);
    }
    let shell_id = shell_id_for(&root_file, &pragmas);
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
