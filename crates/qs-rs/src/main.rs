extern crate quickshell;
extern crate quickshell_io;
extern crate quickshell_wayland;
extern crate quickshell_hyprland;
extern crate quickshell_services_pipewire;
extern crate quickshell_services_upower;
extern crate quickshell_bluetooth;
extern crate quickshell_services_mpris;
extern crate quickshell_services_notifications;
extern crate quickshell_services_systemtray;
extern crate quickshell_services_pam;
extern crate quickshell_dbusmenu;
extern crate quickshell_widgets;

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

const USAGE: &str = "usage: qs-rs [-p <path> | -c <name>] [-n] [ipc (call <target> <function> [args...] | show)]";

/// Resolves the shell file like quickshell: `-p` file or directory, `-c` a named config under
/// `$XDG_CONFIG_HOME/quickshell/`, or a bare path argument. Returns the absolute path as given
/// (what upstream hashes into the shell and path ids) and the real path (what gets scanned).
fn resolve_shell(path: Option<String>, name: Option<String>) -> (PathBuf, PathBuf) {
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
        Ok(real) => (quickshell::instance::absolute_path(&file), real),
        Err(_) => {
            eprintln!("Could not find shell file {}", file.display());
            std::process::exit(1);
        }
    }
}

fn shell_id_for(config: &std::path::Path, pragmas: &launch::Pragmas) -> String {
    pragmas.shell_id.clone().unwrap_or_else(|| quickshell::instance::path_id(&config.to_string_lossy()))
}

/// Newest live instance of this config on this display (upstream selectInstance by path).
fn select_instance(config: &std::path::Path) -> quickshell::instance::InstanceInfo {
    use quickshell::instance;
    let dir = instance::base_run_dir().join("by-path").join(instance::path_id(&config.to_string_lossy()));
    let (live, other, _) = instance::collect(&dir, &instance::display_connection());
    match live.into_iter().next() {
        Some(i) => i,
        None => {
            eprintln!("No running instances for \"{}\"", config.display());
            if !other.is_empty() {
                eprintln!("Found instances on other displays, use --any-display to show them.");
            }
            std::process::exit(255);
        }
    }
}

fn ipc_main(config: &std::path::Path, args: &[String]) -> ! {
    use quickshell_io::ipc::{Command, read_call_reply, read_query_reply, request};
    let id = select_instance(config).instance_id;
    let result = match args.first().map(String::as_str) {
        Some("call") if args.len() >= 3 => {
            let n = args.len() - 3;
            let cmd = Command::Call { target: args[1].clone(), function: args[2].clone(), args: args[3..].to_vec() };
            request(&id, &cmd, |i, r| read_call_reply(i, r, n)).map(|r| r.map(|v| v.unwrap_or_default()))
        }
        Some("show") => request(&id, &Command::Query { target: String::new(), name: String::new() }, read_query_reply),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(1);
        }
    };
    match result {
        Ok(Ok(text)) => {
            if !text.is_empty() {
                println!("{text}");
            }
            std::process::exit(0)
        }
        Ok(Err(msg)) => {
            eprintln!("{msg}");
            std::process::exit(255)
        }
        Err(e) => {
            eprintln!("Could not reach instance {id}: {e}");
            std::process::exit(255)
        }
    }
}

fn main() {
    // PamContext re-executes qs-rs as its authentication helper process.
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some(quickshell_services_pam::helper::HELPER_ARG) {
        std::process::exit(quickshell_services_pam::helper::main(&argv[2..]));
    }
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
    cxx_qt::init_crate!(quickshell_services_systemtray);
    cxx_qt::init_crate!(quickshell_services_pam);
    cxx_qt::init_crate!(quickshell_dbusmenu);
    cxx_qt::init_crate!(quickshell_widgets);

    let mut args = std::env::args().skip(1).peekable();
    let (mut path, mut name, mut rest, mut no_duplicate) = (None, None, Vec::new(), false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-p" | "--path" => path = args.next(),
            "-c" | "--config" => name = args.next(),
            "-n" | "--no-duplicate" => no_duplicate = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            _ if rest.is_empty() && path.is_none() && a.ends_with(".qml") => path = Some(a),
            _ => rest.push(a),
        }
    }
    let (config, root_file) = resolve_shell(path, name);
    let text = std::fs::read_to_string(&root_file).expect("could not read shell file");
    let pragmas = match launch::parse(&text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if rest.first().is_some_and(|a| a == "ipc") {
        ipc_main(&config, &rest[1..]);
    }
    let shell_id = shell_id_for(&config, &pragmas);
    {
        use quickshell::instance;
        let by_path = instance::base_run_dir().join("by-path").join(instance::path_id(&config.to_string_lossy()));
        if no_duplicate && !instance::collect(&by_path, &instance::display_connection()).0.is_empty() {
            eprintln!("An instance of this configuration is already running.");
            std::process::exit(0);
        }
        let info = instance::new_info(&config.to_string_lossy(), &shell_id, pragmas.app_id.as_deref());
        match instance::create(info) {
            Ok(dir) => quickshell_io::ipc::start_server(dir),
            Err(e) => eprintln!("Could not create the instance runtime dir: {e}"),
        }
    }
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
