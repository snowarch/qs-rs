extern crate quickshell;
extern crate quickshell_io;
extern crate quickshell_wayland;
extern crate quickshell_hyprland;
extern crate quickshell_services_pipewire;
extern crate quickshell_services_upower;
extern crate quickshell_bluetooth;
extern crate quickshell_networking;
extern crate quickshell_services_polkit;
extern crate quickshell_services_mpris;
extern crate quickshell_services_notifications;
extern crate quickshell_services_systemtray;
extern crate quickshell_services_pam;
extern crate quickshell_dbusmenu;
extern crate quickshell_widgets;

mod cachesync;
mod cli;
mod command;
mod errors;
mod host;
mod launch;
mod scan;
mod watchfiles;

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

/// Measurement aid: `QSRS_HEAP_DUMP=15,60` writes a jemalloc heap profile at those seconds after
/// start (needs `MALLOC_CONF=prof:true,prof_prefix:...`; read with jeprof).
fn heap_dumps() {
    let Ok(spec) = std::env::var("QSRS_HEAP_DUMP") else { return };
    let mut at: Vec<u64> = spec.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    at.sort_unstable();
    let _ = std::thread::Builder::new().name("qs-heapdump".into()).spawn(move || {
        let start = std::time::Instant::now();
        for s in at {
            std::thread::sleep(std::time::Duration::from_secs(s).saturating_sub(start.elapsed()));
            unsafe { mallctl(c"prof.dump".as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0) };
            let (a, r) = malloc_stats().unwrap_or_default();
            eprintln!("qsrs-mem t={s}s allocated={}MB resident={}MB", a >> 20, r >> 20);
        }
    });
}

/// Image decoders inir can reach (wallpapers: jpg png webp avif bmp svg gif tiff jxl; PNG and BMP
/// are built into Qt). Any image without a known suffix makes Qt load every decoder installed to
/// probe it, and KDE's set pulls in video codecs, RAW, HEIF, EXR and more.
const IMAGE_FORMATS: &[&str] = &["libqjpeg", "libqgif", "libqwebp", "libqsvg", "libqtiff", "libqavif6", "libqico", "kimg_jxl"];

/// A plugin dir that mirrors Qt's with only `IMAGE_FORMATS` under imageformats, built once per
/// Qt plugin dir and reused. `QSRS_ALL_IMAGE_FORMATS=1` keeps Qt's own.
fn curated_plugins() -> Option<PathBuf> {
    if std::env::var_os("QSRS_ALL_IMAGE_FORMATS").is_some() {
        return None;
    }
    let qt = PathBuf::from(host::ffi::qt_plugins_path());
    let key = quickshell::instance::path_id(&format!("{}{IMAGE_FORMATS:?}", qt.display()));
    let dir = runtime_dir().join(format!("plugins-{key}"));
    if dir.is_dir() {
        return Some(dir);
    }
    let tmp = runtime_dir().join(format!("plugins-{key}.{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("imageformats")).ok()?;
    for entry in std::fs::read_dir(&qt).ok()?.flatten() {
        let name = entry.file_name();
        if name == "imageformats" {
            for f in std::fs::read_dir(entry.path()).ok()?.flatten() {
                let stem = f.path().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                if IMAGE_FORMATS.contains(&stem.as_str()) {
                    std::os::unix::fs::symlink(f.path(), tmp.join("imageformats").join(f.file_name())).ok()?;
                }
            }
        } else {
            std::os::unix::fs::symlink(entry.path(), tmp.join(&name)).ok()?;
        }
    }
    // Another instance may have won the race; either copy is the same.
    if std::fs::rename(&tmp, &dir).is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    dir.is_dir().then_some(dir)
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

fn shell_id_for(config: &std::path::Path, pragmas: &launch::Pragmas) -> String {
    pragmas.shell_id.clone().unwrap_or_else(|| quickshell::instance::path_id(&config.to_string_lossy()))
}

/// `-d`: write end of the pipe the waiting parent reads the launch result from.
static DAEMON_PIPE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// Forks like upstream's `--daemonize`: the child leaves the session and goes on; the parent
/// exits with the code the child reports once the config is loaded (or it failed).
fn daemonize() {
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == -1 {
        eprintln!("Failed to create messaging pipes for daemon: {}", std::io::Error::last_os_error());
        std::process::exit(1);
    }
    match unsafe { libc::fork() } {
        -1 => {
            eprintln!("Failed to fork daemon: {}", std::io::Error::last_os_error());
            std::process::exit(1);
        }
        0 => {
            unsafe { libc::close(fds[0]) };
            DAEMON_PIPE.store(fds[1], std::sync::atomic::Ordering::Relaxed);
            if unsafe { libc::setsid() } == -1 {
                eprintln!("Failed to setsid: {}", std::io::Error::last_os_error());
                std::process::exit(1);
            }
            // jemalloc stops its purge thread in a forked child; start it again.
            let mut on = true;
            unsafe { mallctl(c"background_thread".as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), (&raw mut on).cast(), size_of::<bool>()) };
        }
        _ => {
            unsafe { libc::close(fds[1]) };
            let mut code: i32 = 0;
            let n = unsafe { libc::read(fds[0], (&raw mut code).cast(), size_of::<i32>()) };
            if n != size_of::<i32>() as isize {
                // Upstream exits 0 here; a child that died before reporting is a failure.
                eprintln!("Failed to wait for daemon launch (it may have crashed)");
                std::process::exit(1);
            }
            std::process::exit(code);
        }
    }
}

/// Reports `code` to the parent of a daemonized instance and detaches stdio (upstream
/// exitDaemon). No-op otherwise or when already reported.
fn exit_daemon(code: i32) {
    let fd = DAEMON_PIPE.swap(-1, std::sync::atomic::Ordering::Relaxed);
    if fd == -1 {
        return;
    }
    unsafe {
        libc::write(fd, (&raw const code).cast(), size_of::<i32>());
        libc::close(fd);
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if null != -1 {
            for std_fd in 0..3 {
                libc::dup2(null, std_fd);
            }
            libc::close(null);
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
    // upstream's binary is `qs`: the cargo artifact `qs-rs` answers to it. A link of another name
    // keeps its own (inir runs the shell as `inir-shell`, out of CachyOS's ananicy rule that nices
    // every `qs` to 10). Threads started later inherit it. Mesa app profiles match the executable
    // name, not this.
    if std::fs::read_to_string("/proc/self/comm").is_ok_and(|c| c.trim() == "qs-rs") {
        let _ = std::fs::write("/proc/self/comm", "qs");
    }
    let cli = match cli::parse(argv.into_iter().skip(1)) {
        Ok(c) => c,
        Err(cli::Exit { code: 0, text }) => {
            println!("{text}");
            return;
        }
        Err(cli::Exit { code, text }) => {
            eprintln!("{text}");
            std::process::exit(code);
        }
    };
    if cli.check_compat {
        if let Some((running, built)) = command::qt_mismatch() {
            println!(
                "\x1b[31mCOMPATIBILITY WARNING: qs-rs was built against Qt {built} but the system has updated to Qt {running} \
                 without rebuilding the package. This is likely to cause crashes, so you must rebuild qs-rs.\n\x1b[0m"
            );
            std::process::exit(1);
        }
        return;
    }
    let launching = !cli.version && cli.cmd == cli::Command::Launch;
    // Before the fork: an instance already running ends here, like upstream's child would.
    let located = if launching || (!cli.version && cli.cmd == cli::Command::Check) {
        match command::locate_config_file(&cli) {
            Ok(c) => Some(c),
            Err(()) => std::process::exit(255),
        }
    } else {
        None
    };
    if launching && cli.no_duplicate && command::select_instance(&cli, false, false).is_ok() {
        println!("An instance of this configuration is already running.");
        return;
    }
    // Has to happen before Qt or any thread of ours starts.
    if cli.daemonize && launching {
        daemonize();
    }
    let color = !cli.no_color && std::env::var("NO_COLOR").map_or(true, |v| v.is_empty());
    quickshell::log::configure(&cli.log_rules, cli.verbosity, color, cli.log_times, cli.sparse);
    let code = match &cli.cmd {
        _ if cli.version => command::print_version(cli.verbosity),
        cli::Command::Log { file, tail, follow, rules } => command::log(&cli, file.as_deref(), *tail, *follow, rules),
        cli::Command::Errors { follow, static_check } => command::errors(&cli, *follow, *static_check),
        cli::Command::Frames { action, arg } => command::frames(&cli, action, arg),
        cli::Command::List => command::list(&cli),
        cli::Command::Kill => command::kill(&cli),
        cli::Command::Ipc(ipc) => command::ipc(&cli, ipc),
        cli::Command::Launch | cli::Command::Check => run_shell(&cli, located.unwrap(), started),
    };
    exit_daemon(code);
    std::process::exit(code);
}

fn run_shell(cli: &cli::Cli, config: PathBuf, started: std::time::Instant) -> i32 {
    cxx_qt::init_crate!(quickshell);
    cxx_qt::init_crate!(quickshell_io);
    cxx_qt::init_crate!(quickshell_wayland);
    cxx_qt::init_crate!(quickshell_hyprland);
    cxx_qt::init_crate!(quickshell_services_pipewire);
    cxx_qt::init_crate!(quickshell_services_upower);
    cxx_qt::init_crate!(quickshell_bluetooth);
    cxx_qt::init_crate!(quickshell_networking);
    cxx_qt::init_crate!(quickshell_services_polkit);
    cxx_qt::init_crate!(quickshell_services_mpris);
    cxx_qt::init_crate!(quickshell_services_notifications);
    cxx_qt::init_crate!(quickshell_services_systemtray);
    cxx_qt::init_crate!(quickshell_services_pam);
    cxx_qt::init_crate!(quickshell_dbusmenu);
    cxx_qt::init_crate!(quickshell_widgets);
    quickshell::common::capture_initial_env();
    host::ffi::install_message_handler();
    host::ffi::install_crash_handler();
    let root_file = match std::fs::canonicalize(&config) {
        Ok(real) => real,
        Err(e) => {
            eprintln!("Could not open config file at {:?}: {e}", config.display().to_string());
            return 255;
        }
    };
    let text = match std::fs::read_to_string(&root_file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Could not read {:?}: {e}", root_file.display().to_string());
            return 255;
        }
    };
    let mut pragmas = match launch::parse(&text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    // Upstream launch.cpp: QS_APP_ID and QS_ICON_THEME are the defaults the pragmas override.
    pragmas.app_id = pragmas.app_id.take().or_else(|| std::env::var("QS_APP_ID").ok().filter(|v| !v.is_empty()));
    pragmas.icon_theme = pragmas.icon_theme.take().or_else(|| std::env::var("QS_ICON_THEME").ok().filter(|v| !v.is_empty()));
    let shell_id = shell_id_for(&config, &pragmas);
    // `check`: compile every QML file the shell reaches, report errors, start nothing.
    let check = cli.cmd == cli::Command::Check;
    if check && std::env::var_os("QT_QPA_PLATFORM").is_none() {
        unsafe { std::env::set_var("QT_QPA_PLATFORM", "offscreen") };
    }
    // A check compiles from a private VFS path, so its cache entries could never be hit again.
    if check {
        unsafe { std::env::set_var("QML_DISABLE_DISK_CACHE", "1") };
    }
    let mut run_dir = None;
    if !check {
        if let Some((running, built)) = command::qt_mismatch() {
            quickshell::qs_log!(
                warn,
                "default",
                "\x1b[31mqs-rs was built against Qt {built} but the system has updated to Qt {running} without rebuilding the package. \
                 This is likely to cause crashes, so qs-rs must be rebuilt.\n"
            );
        }
        use quickshell::instance;
        let info = instance::new_info(&config.to_string_lossy(), &shell_id, pragmas.app_id.as_deref());
        match instance::create(info) {
            Ok(dir) => {
                run_dir = Some(dir);
                if let Err(e) = quickshell::log::init_files(dir) {
                    eprintln!("Could not create the instance log files: {e}");
                }
                let _ = quickshell_io::ipc::FRAMES_HOOK.set(|action, arg| host::ffi::frames_command(action, arg));
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
    if !check {
        quickshell::errorlog::set_root(&root_file);
    }
    launch::apply_env(&pragmas);
    if let Some(dir) = cachesync::qt_cache_dir() {
        cachesync::init(dir);
    }
    if let Some(dir) = run_dir {
        launch::drop_expensive_fonts(&pragmas, dir);
    }

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

    heap_dumps();
    if let Some(dir) = curated_plugins() {
        host::ffi::set_plugin_path(&qstr(&dir));
    }
    host::ffi::setup_icons(&QString::from(pragmas.icon_theme.as_deref().unwrap_or_default()));
    if pragmas.enable_webengine {
        host::ffi::init_webengine();
    }
    let mut app = host::ffi::create_app(pragmas.use_qapplication, pragmas.desktop_settings_aware);
    if pragmas.enable_webengine {
        host::ffi::init_webengine();
    }
    host::ffi::configure_app(&QString::from(pragmas.app_id.as_deref().unwrap_or("org.quickshell")), pragmas.native_text_rendering);
    // QML debug server for qmlprofiler/GammaRay before the engine: upstream's `--debug PORT
    // [--waitfordebug]`, or QSRS_QML_DEBUG=PORT[,block].
    let debug = cli.debug_port.map(|p| (p.to_string(), cli.wait_debug)).or_else(|| {
        let spec = std::env::var("QSRS_QML_DEBUG").ok()?;
        Some(spec.split_once(',').map_or((spec.clone(), false), |(p, m)| (p.to_string(), m == "block")))
    });
    if let Some((port, block)) = debug {
        if let Ok(port) = port.parse() {
            if !host::ffi::start_qml_debug_server(port, block) {
                eprintln!("qs-rs: QML debug server failed on port {port}");
            }
        }
    }
    let engine = host::ffi::new_engine(&qstr(&vfs_root));
    if check {
        let code = check_shell(&scan, &vfs, unsafe { std::pin::Pin::new_unchecked(&mut *engine) });
        let _ = std::fs::remove_dir_all(&vfs_root);
        return code;
    }
    quickshell::common::set_reload_hook(host::ffi::request_reload);
    let root = unsafe { host::ffi::create_root(std::pin::Pin::new_unchecked(&mut *engine), &qstr(&entry)) };
    if !root.is_null() {
        let _ = host::SHELL.set((root_file.clone(), vfs.clone()));
        unsafe { host::ffi::install_reload(engine, root, &qstr(&vfs_root), &qstr(&entry)) };
        watchfiles::track(scan.hashes);
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
    let code = if root.is_null() {
        1
    } else {
        exit_daemon(0);
        host::ffi::install_quit_signals();
        host::ffi::watch_memory_peaks();
        let code = host::ffi::exec_app(app.pin_mut());
        quickshell_wayland::windowcast::stop_all(std::time::Duration::from_millis(500));
        code
    };
    if _vfs_lock.is_none() {
        let _ = std::fs::remove_dir_all(&vfs_root);
    }
    code
}
