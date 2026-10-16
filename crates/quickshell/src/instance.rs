//! Quickshell's per-instance runtime layout (upstream core/paths.cpp, launch/launch.cpp), so the
//! upstream `qs` CLI (`qs -p <dir> ipc call`, `qs list`) finds and talks to qs-rs:
//!
//! `$XDG_RUNTIME_DIR/quickshell/by-id/<instance>/{instance.lock,ipc.sock}`, with symlinks
//! `by-shell/<shell id>/<instance>` and `by-pid/<pid>` to it and `by-path/<md5 of the config
//! path>` to the shell dir. `instance.lock` holds a write lock for the process lifetime and the
//! QDataStream-encoded InstanceInfo.

use crate::datastream::{Incomplete, Reader, Writer, read_datetime, write_local_datetime};
use cxx_qt_lib::QString;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstanceInfo {
    pub instance_id: String,
    pub config_path: String,
    pub shell_id: String,
    pub app_id: String,
    pub launch_time_ms: i64,
    pub pid: i32,
    pub display: String,
}

impl InstanceInfo {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.string(&self.instance_id).string(&self.config_path).string(&self.shell_id).string(&self.app_id);
        write_local_datetime(&mut w, self.launch_time_ms);
        w.i32(self.pid).string(&self.display);
        w.0
    }

    pub fn decode(data: &[u8]) -> Result<Self, Incomplete> {
        let mut r = Reader::new(data);
        Ok(Self {
            instance_id: r.string()?,
            config_path: r.string()?,
            shell_id: r.string()?,
            app_id: r.string()?,
            launch_time_ms: read_datetime(&mut r)?,
            pid: r.i32()?,
            display: r.string()?,
        })
    }
}

/// `$XDG_RUNTIME_DIR/quickshell` (or `/run/user/<uid>/quickshell`).
pub fn base_run_dir() -> PathBuf {
    let runtime = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })),
    };
    runtime.join("quickshell")
}

pub fn ipc_path(instance_id: &str) -> PathBuf {
    base_run_dir().join("by-id").join(instance_id).join("ipc.sock")
}

/// Same string upstream records and filters instances by.
pub fn display_connection() -> String {
    let get = |v| std::env::var(v).ok().filter(|s| !s.is_empty());
    match (get("WAYLAND_DISPLAY"), get("DISPLAY")) {
        (Some(w), _) => format!("wayland/{w}"),
        (None, Some(x)) => format!("x11/{x}"),
        _ => "unk".into(),
    }
}

/// Absolute path without resolving symlinks or `..` through them (QFileInfo::absoluteFilePath),
/// which is what upstream hashes into the path id.
pub fn absolute_path(path: &Path) -> PathBuf {
    let joined = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(path) };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

pub fn path_id(config_path: &str) -> String {
    crate::util::md5_hex(&QString::from(config_path)).to_string()
}

/// Least significant digit first, as upstream's base36Encode.
fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut s = String::new();
    loop {
        s.push(DIGITS[(n % 36) as usize] as char);
        n /= 36;
        if n == 0 {
            return s;
        }
    }
}

pub fn new_info(config_path: &str, shell_id: &str, app_id: Option<&str>) -> InstanceInfo {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let pid = std::process::id();
    InstanceInfo {
        instance_id: base36(pid as u64) + &base36(now.as_secs()),
        config_path: config_path.into(),
        shell_id: shell_id.into(),
        app_id: app_id.unwrap_or("org.quickshell").into(),
        launch_time_ms: now.as_millis() as i64,
        pid: pid as i32,
        display: display_connection(),
    }
}

struct Current {
    info: InstanceInfo,
    dir: PathBuf,
    _lock: File,
}

static CURRENT: OnceLock<Current> = OnceLock::new();

pub fn current() -> Option<(&'static InstanceInfo, &'static Path)> {
    CURRENT.get().map(|c| (&c.info, c.dir.as_path()))
}

fn relink(target: &Path, link: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(link);
    std::os::unix::fs::symlink(target, link)
}

/// fcntl write lock over the whole file: F_SETLK takes it, F_GETLK reports who holds it.
fn write_lock(file: &File, cmd: libc::c_int) -> (libc::c_int, libc::flock) {
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = libc::F_WRLCK as libc::c_short;
    lock.l_whence = libc::SEEK_SET as libc::c_short;
    let ret = unsafe { libc::fcntl(file.as_raw_fd(), cmd, &mut lock) };
    (ret, lock)
}

/// Creates this process's instance dir, lock and links. Returns the instance dir.
pub fn create(info: InstanceInfo) -> std::io::Result<&'static Path> {
    let base = base_run_dir();
    let dir = base.join("by-id").join(&info.instance_id);
    std::fs::create_dir_all(&dir)?;
    let mut lock = File::options().read(true).write(true).create(true).truncate(true).open(dir.join("instance.lock"))?;
    if write_lock(&lock, libc::F_SETLK).0 != 0 {
        return Err(std::io::Error::last_os_error());
    }
    lock.write_all(&info.encode())?;
    lock.flush()?;
    let canonical = std::fs::canonicalize(&dir)?;
    let shell_dir = base.join("by-shell").join(&info.shell_id);
    std::fs::create_dir_all(&shell_dir)?;
    relink(&canonical, &shell_dir.join(&info.instance_id))?;
    std::fs::create_dir_all(base.join("by-pid"))?;
    relink(&canonical, &base.join("by-pid").join(info.pid.to_string()))?;
    std::fs::create_dir_all(base.join("by-path"))?;
    relink(&std::fs::canonicalize(&shell_dir)?, &base.join("by-path").join(path_id(&info.config_path)))?;
    let _ = CURRENT.set(Current { info, dir, _lock: lock });
    Ok(&CURRENT.get().unwrap().dir)
}

/// The shell's VFS dir, `<run>/vfs/<shell id>` like upstream, with `tooling.lock` held for the
/// life of the process (keep the returned file). Err when another instance holds it.
pub fn lock_vfs(shell_id: &str) -> std::io::Result<(PathBuf, File)> {
    let dir = base_run_dir().join("vfs").join(shell_id);
    std::fs::create_dir_all(&dir)?;
    let lock = File::options().read(true).write(true).create(true).truncate(false).open(dir.join("tooling.lock"))?;
    if write_lock(&lock, libc::F_SETLK).0 != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((dir, lock))
}

/// An instance dir's lock: its InstanceInfo and the pid holding the lock (-1 when dead).
pub fn check_lock(dir: &Path) -> Option<(i32, InstanceInfo)> {
    let mut file = File::open(dir.join("instance.lock")).ok()?;
    let (_, lock) = write_lock(&file, libc::F_GETLK);
    let pid = if lock.l_type == libc::F_UNLCK as libc::c_short { -1 } else { lock.l_pid };
    let mut data = Vec::new();
    file.read_to_end(&mut data).ok()?;
    Some((pid, InstanceInfo::decode(&data).ok()?))
}

/// Instances under a by-path/by-shell dir: (live on `display` or any display when empty,
/// live on another display, dead), like upstream collectInstances.
pub fn collect(dir: &Path, display: &str) -> (Vec<InstanceInfo>, Vec<InstanceInfo>, Vec<InstanceInfo>) {
    let (mut live, mut other, mut dead) = (vec![], vec![], vec![]);
    let Ok(entries) = std::fs::read_dir(dir) else { return (live, other, dead) };
    for e in entries.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let Some((pid, info)) = check_lock(&e.path()) else { continue };
        if pid == -1 {
            dead.push(info);
        } else if !display.is_empty() && info.display != display {
            other.push(info);
        } else {
            live.push(info);
        }
    }
    live.sort_by_key(|i| std::cmp::Reverse(i.launch_time_ms));
    (live, other, dead)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base36_is_least_significant_first() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(36), "01");
        assert_eq!(base36(37 * 36 + 35), "z11");
    }

    #[test]
    fn info_round_trips() {
        let info = InstanceInfo {
            instance_id: "abc".into(),
            config_path: "/x/shell.qml".into(),
            shell_id: "inir".into(),
            app_id: "org.quickshell".into(),
            launch_time_ms: 1_760_000_000_000,
            pid: 4242,
            display: "wayland/wayland-1".into(),
        };
        assert_eq!(InstanceInfo::decode(&info.encode()).unwrap(), info);
    }

    #[test]
    fn absolute_keeps_symlinks_and_drops_dots() {
        assert_eq!(absolute_path(Path::new("/a/./b/../c/shell.qml")), PathBuf::from("/a/c/shell.qml"));
    }
}
