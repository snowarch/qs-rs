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

/// jemalloc's defaults when `MALLOC_CONF` is unset: inir's tuning (two arenas, 1 s dirty decay,
/// a background purge thread) so freed memory goes back to the system instead of staying in
/// per-core arenas.
#[repr(transparent)]
pub struct MallocConf(*const u8);
unsafe impl Sync for MallocConf {}
#[unsafe(no_mangle)]
#[used]
pub static malloc_conf: MallocConf =
    MallocConf(c"background_thread:true,narenas:2,dirty_decay_ms:1000,muzzy_decay_ms:0,tcache_max:4096".as_ptr().cast());

unsafe extern "C" {
    fn mallctl(name: *const std::ffi::c_char, old: *mut u8, oldlen: *mut usize, new: *mut u8, newlen: usize) -> i32;
}

/// Bytes jemalloc holds for live allocations and as resident pages (`stats.allocated`,
/// `stats.resident`), refreshed through `epoch`.
pub fn malloc_stats() -> Option<(usize, usize)> {
    unsafe {
        let mut epoch: u64 = 1;
        let mut len = size_of::<u64>();
        mallctl(c"epoch".as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), (&raw mut epoch).cast(), len);
        let mut read = |name: &std::ffi::CStr| {
            let mut v: usize = 0;
            len = size_of::<usize>();
            (mallctl(name.as_ptr(), (&raw mut v).cast(), &mut len, std::ptr::null_mut(), 0) == 0).then_some(v)
        };
        Some((read(c"stats.allocated")?, read(c"stats.resident")?))
    }
}

/// Compiles every QML file of the scan through the VFS and prints each distinct error once.
fn check_shell(scan: &scan::Scan, vfs: &std::path::Path, mut engine: std::pin::Pin<&mut cxx_qt_lib::QQmlEngine>) -> i32 {
    let mut errors = std::collections::BTreeSet::new();
    let mut files = 0;
    for file in scan.files.iter().filter(|f| f.extension().is_some_and(|e| e == "qml")) {
        let Ok(rel) = file.strip_prefix(&scan.root) else { continue };
        files += 1;
        let out = host::ffi::check_file(engine.as_mut(), &qstr(&vfs.join(rel)));
        let prefix = format!("file://{}/", vfs.display());
        errors.extend(out.lines().filter(|l| !l.is_empty()).map(|l| l.replace(&prefix, "")));
    }
    for e in &errors {
        println!("{e}");
    }
    println!("{files} files checked, {} errors", errors.len());
    i32::from(!errors.is_empty())
}

/// Private VFS dirs (`vfs-<pid>`) left behind by instances that were killed.
fn clean_stale_vfs() {
    for entry in std::fs::read_dir(runtime_dir()).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.strip_prefix("vfs-")) else { continue };
        if pid.parse::<u32>().is_ok() && !std::path::Path::new("/proc").join(pid).exists() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn qstr(p: &std::path::Path) -> QString {
    QString::from(&*p.to_string_lossy())
}

const USAGE: &str = "usage: qs-rs [-p <path> | -c <name>] [-n] [-v] [--no-color] [--log-times] [--log-rules <rules>]\n\
    [ipc (call <target> <function> [args...] | show) | check | log [file] [-t <n>] [-f] [-r <rules>]]";

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

/// `log`: decodes the newest instance's (live, else dead) detailed log, or a given file.
fn log_main(config: Option<&std::path::Path>, args: &[String], mut color: bool, mut timestamps: bool) -> ! {
    use quickshell::instance;
    let (mut file, mut tail, mut follow, mut rules) = (None, 0usize, false, String::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-t" | "--tail" => tail = it.next().and_then(|n| n.parse().ok()).unwrap_or(0),
            "-f" | "--follow" => follow = true,
            "--no-color" => color = false,
            "--log-times" => timestamps = true,
            "-r" | "--rules" => rules = it.next().cloned().unwrap_or_default(),
            _ if a.starts_with("--rules=") => rules = a["--rules=".len()..].to_string(),
            _ if file.is_none() && !a.starts_with('-') => file = Some(std::path::PathBuf::from(a)),
            _ => {
                eprintln!("{USAGE}");
                std::process::exit(1);
            }
        }
    }
    let path = match (file, config) {
        (Some(f), _) => f,
        (None, Some(config)) => {
            let dir = instance::base_run_dir().join("by-path").join(instance::path_id(&config.to_string_lossy()));
            let (live, _, mut dead) = instance::collect(&dir, &instance::display_connection());
            dead.sort_by_key(|i| std::cmp::Reverse(i.launch_time_ms));
            match live.into_iter().next().or_else(|| dead.into_iter().next()) {
                Some(i) => instance::base_run_dir().join("by-id").join(i.instance_id).join("log.qslog"),
                None => {
                    eprintln!("No running instances for \"{}\"", config.display());
                    std::process::exit(255);
                }
            }
        }
        (None, None) => {
            eprintln!("{USAGE}");
            std::process::exit(1);
        }
    };
    std::process::exit(quickshell::log::read_file(&path, tail, follow, &rules, color, timestamps));
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
    let started = std::time::Instant::now();
    // PamContext re-executes qs-rs as its authentication helper process.
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some(quickshell_services_pam::helper::HELPER_ARG) {
        std::process::exit(quickshell_services_pam::helper::main(&argv[2..]));
    }
    // Shells find their instance by process name (`pgrep -x qs`, inir's close-window.sh), as
    // upstream's binary is `qs`. Threads started later inherit it. Mesa app profiles match the
    // executable name, not this.
    let _ = std::fs::write("/proc/self/comm", "qs");
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
    let (mut verbosity, mut color, mut log_times, mut log_rules) = (0u32, true, false, String::new());
    while let Some(a) = args.next() {
        match a.as_str() {
            "-p" | "--path" => path = args.next(),
            "-c" | "--config" => name = args.next(),
            "-n" | "--no-duplicate" => no_duplicate = true,
            "-v" | "--verbose" => verbosity += 1,
            "-vv" => verbosity += 2,
            "--no-color" => color = false,
            "--log-times" => log_times = true,
            "--log-rules" => log_rules = args.next().unwrap_or_default(),
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            _ if rest.is_empty() && path.is_none() && a.ends_with(".qml") => path = Some(a),
            _ => {
                rest.push(a);
                // Everything after a subcommand belongs to it.
                rest.extend(args.by_ref());
            }
        }
    }
    quickshell::log::configure(&log_rules, verbosity, color, log_times);
    if rest.first().is_some_and(|a| a == "log") {
        let config = (path.is_some() || name.is_some()).then(|| resolve_shell(path, name).0);
        log_main(config.as_deref(), &rest[1..], color, log_times);
    }
    host::ffi::install_message_handler();
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
    // `check`: compile every QML file the shell reaches, report errors, start nothing.
    let check = rest.first().is_some_and(|a| a == "check");
    if check && std::env::var_os("QT_QPA_PLATFORM").is_none() {
        unsafe { std::env::set_var("QT_QPA_PLATFORM", "offscreen") };
    }
    if !check {
        use quickshell::instance;
        let by_path = instance::base_run_dir().join("by-path").join(instance::path_id(&config.to_string_lossy()));
        if no_duplicate && !instance::collect(&by_path, &instance::display_connection()).0.is_empty() {
            eprintln!("An instance of this configuration is already running.");
            std::process::exit(0);
        }
        let info = instance::new_info(&config.to_string_lossy(), &shell_id, pragmas.app_id.as_deref());
        match instance::create(info) {
            Ok(dir) => {
                if let Err(e) = quickshell::log::init_files(dir) {
                    eprintln!("Could not create the instance log files: {e}");
                }
                quickshell_io::ipc::start_server(dir)
            }
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

    // The VFS lives at a stable path like upstream's, so Qt's QML disk cache (keyed by file
    // path) hits across starts. A second instance of the shell gets a private one.
    clean_stale_vfs();
    let (vfs_root, _vfs_lock) = match quickshell::instance::lock_vfs(&quickshell::common::shell().shell_id) {
        Ok((dir, lock)) if !check => (dir, Some(lock)),
        _ if check => (runtime_dir().join(format!("vfs-{}", std::process::id())), None),
        _ => {
            eprintln!("The VFS of this shell is used by another instance; using a private one.");
            (runtime_dir().join(format!("vfs-{}", std::process::id())), None)
        }
    };
    let scan = scan::Scan::new(&root_file).expect("scan failed");
    let vfs = vfs_root.join("qs");
    scan.write_vfs(&vfs).expect("vfs write failed");
    let entry = vfs.join(root_file.strip_prefix(&scan.root).unwrap());

    let mut app = host::ffi::create_app(pragmas.use_qapplication, pragmas.desktop_settings_aware);
    let engine = host::ffi::new_engine(&qstr(&vfs_root));
    if check {
        let code = check_shell(&scan, &vfs, unsafe { std::pin::Pin::new_unchecked(&mut *engine) });
        let _ = std::fs::remove_dir_all(&vfs_root);
        std::process::exit(code);
    }
    let root = unsafe { host::ffi::create_root(std::pin::Pin::new_unchecked(&mut *engine), &qstr(&entry)) };
    if !root.is_null() {
        quickshell::log::emit(quickshell::log::Level::Info, "default", "Configuration Loaded".into());
        let (alloc, resident) = malloc_stats().unwrap_or_default();
        quickshell::qs_log!(
            info,
            "quickshell.qsrs",
            "Configuration loaded in {} ms ({} MB allocated, {} MB resident)",
            started.elapsed().as_millis(),
            alloc >> 20,
            resident >> 20
        );
    }
    let code = if root.is_null() { 1 } else { host::ffi::exec_app(app.pin_mut()) };
    if _vfs_lock.is_none() {
        let _ = std::fs::remove_dir_all(&vfs_root);
    }
    std::process::exit(code);
}
