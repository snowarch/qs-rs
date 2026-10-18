//! Process-wide state shared by the Quickshell modules: the initial environment and the shell's
//! identity and directories, fixed at launch before the QML engine exists.

use std::path::PathBuf;
use std::sync::OnceLock;

static INITIAL_ENV: OnceLock<Vec<(String, String)>> = OnceLock::new();
static SHELL: OnceLock<ShellInfo> = OnceLock::new();

/// Snapshot the environment before shell pragmas modify it. Child processes start from this.
pub fn capture_initial_env() {
    INITIAL_ENV.get_or_init(|| std::env::vars().collect());
}

pub fn initial_env() -> &'static [(String, String)] {
    INITIAL_ENV.get_or_init(|| std::env::vars().collect())
}

#[derive(Debug, Default, Clone)]
pub struct ShellInfo {
    pub shell_id: String,
    /// Directory of the root shell file (real path, not the vfs mirror).
    pub shell_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
}

pub fn set_shell(info: ShellInfo) {
    let _ = SHELL.set(info);
}

pub fn shell() -> &'static ShellInfo {
    SHELL.get_or_init(ShellInfo::default)
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback),
    }
}

/// `$XDG_*_HOME/quickshell/by-shell/<id>`, or a pragma override where `$BASE` is `$XDG_*_HOME`.
pub fn shell_dir(var: &str, fallback: &str, shell_id: &str, override_: Option<&str>) -> PathBuf {
    let base = xdg(var, fallback);
    match override_ {
        Some(o) => PathBuf::from(o.replace("$BASE", &base.to_string_lossy())),
        None => base.join("quickshell/by-shell").join(shell_id),
    }
}

/// `Quickshell.watchFiles`, process-wide like upstream's QuickshellSettings: a reload makes a new
/// singleton, the setting stays.
static WATCH_FILES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn watch_files() -> bool {
    WATCH_FILES.load(std::sync::atomic::Ordering::Relaxed) && std::env::var_os("QS_DISABLE_FILE_WATCHER").is_none_or(|v| v.is_empty())
}

/// Returns whether the value changed.
pub fn set_watch_files(on: bool) -> bool {
    WATCH_FILES.swap(on, std::sync::atomic::Ordering::Relaxed) != on
}

type ReloadHook = Box<dyn Fn(bool) + Send + Sync>;
static RELOAD: OnceLock<ReloadHook> = OnceLock::new();

/// The host installs how `Quickshell.reload()` is carried out.
pub fn set_reload_hook(f: impl Fn(bool) + Send + Sync + 'static) {
    let _ = RELOAD.set(Box::new(f));
}

pub fn request_reload(hard: bool) {
    match RELOAD.get() {
        Some(f) => f(hard),
        None => crate::qs_log!(warn, "quickshell", "Quickshell.reload(): reload is not available"),
    }
}
