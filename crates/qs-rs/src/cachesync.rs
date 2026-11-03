//! Qt writes each compiled QML file into its disk cache through QSaveFile, which flushes it with
//! fdatasync: one blocking flush per file, ~850 for inir, 4.3 s of a 6.9 s cold start (measured
//! with the flushes stubbed out: 2.6 s). qs-rs interposes fsync/fdatasync. A flush of a file in
//! the cache dir returns at once, a thread syncs the filesystem once the writes stop, and a durable
//! `.dirty` marker covers the gap: a run that dies before that sync leaves the marker, and the next
//! start drops the cache instead of mapping torn files. Every other flush goes to the kernel.
//! Qt also never deletes an entry, and each path the shell was loaded from leaves ~900 behind
//! (194 MB here, 856 in use): a while after start, entries this process maps get their mtime
//! bumped, so mtime means "last used", and entries unused for a week go.

use std::ffi::c_int;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

const MARKER: &str = ".dirty";
/// Quiet time after the last skipped flush before the one real sync.
const SETTLE: Duration = Duration::from_millis(1500);
/// When the shell has loaded what it loads at start, and how long an unused entry stays.
const PRUNE_AFTER: Duration = Duration::from_secs(120);
const KEEP: Duration = Duration::from_secs(7 * 24 * 3600);

static DIR: OnceLock<PathBuf> = OnceLock::new();
static STATE: Mutex<State> = Mutex::new(State { dirty: false, generation: 0, syncer: false });
static WAKE: Condvar = Condvar::new();

struct State {
    /// The marker is on disk: some cache write since the last sync is not durable yet.
    dirty: bool,
    /// Bumped by every skipped flush, so the syncer knows whether more came in while it slept.
    generation: u64,
    syncer: bool,
}

/// Qt's default cache dir (QStandardPaths::CacheLocation + "/qmlcache", app name "quickshell",
/// no organization), or QML_DISK_CACHE_PATH when the user set one.
pub fn qt_cache_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("QML_DISK_CACHE_PATH").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("quickshell/qmlcache"))
}

/// Call before the engine loads anything. Drops a cache the last run left unflushed.
pub fn init(dir: PathBuf) {
    if dir.join(MARKER).exists() {
        quickshell::qs_log!(info, "qs.cache", "QML cache was not flushed by the last run; recompiling");
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = DIR.set(dir.clone());
    let _ = std::thread::Builder::new().name("qs-cacheprune".into()).spawn(move || {
        std::thread::sleep(PRUNE_AFTER);
        prune(&dir, KEEP);
    });
}

/// The shell is quitting cleanly: sync what still waits for its quiet time and drop the marker, so a shell that exits
/// right after writing (a restart, an update, a test) keeps its cache on the next start instead of compiling it all.
pub fn flush() {
    let Some(dir) = DIR.get() else { return };
    let mut st = STATE.lock().unwrap();
    if !st.dirty {
        return;
    }
    if let Ok(d) = std::fs::File::open(dir) {
        use std::os::fd::AsRawFd;
        unsafe { libc::syncfs(d.as_raw_fd()) };
    }
    let _ = std::fs::remove_file(dir.join(MARKER));
    st.dirty = false;
    st.generation += 1;
}

fn prune(dir: &Path, keep: Duration) {
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    let used: std::collections::HashSet<&Path> =
        maps.lines().filter_map(|l| l.find('/').map(|i| Path::new(&l[i..]))).filter(|p| p.starts_with(dir)).collect();
    let now = std::time::SystemTime::now();
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let (mut kept, mut dropped) = (0, 0);
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "qmlc") {
            continue;
        }
        if used.contains(path.as_path()) {
            if let Ok(f) = std::fs::File::open(&path) {
                let _ = f.set_modified(now);
            }
            kept += 1;
        } else if e.metadata().and_then(|m| m.modified()).is_ok_and(|t| now.duration_since(t).unwrap_or_default() > keep) {
            dropped += usize::from(std::fs::remove_file(&path).is_ok());
        }
    }
    quickshell::qs_log!(debug, "qs.cache", "QML cache: {kept} entries in use, {dropped} unused ones dropped");
}

fn in_cache(fd: c_int) -> Option<&'static Path> {
    let dir = DIR.get()?;
    let target = std::fs::read_link(format!("/proc/self/fd/{fd}")).ok()?;
    let rest = target.as_os_str().as_bytes().strip_prefix(dir.as_os_str().as_bytes())?;
    (rest.first() == Some(&b'/') && !target.ends_with(MARKER)).then_some(dir)
}

fn real_fsync(fd: c_int) -> c_int {
    unsafe { libc::syscall(libc::SYS_fsync, fd as libc::c_long) as c_int }
}

fn real_fdatasync(fd: c_int) -> c_int {
    unsafe { libc::syscall(libc::SYS_fdatasync, fd as libc::c_long) as c_int }
}

/// Makes the marker durable before the first skipped flush returns, then hands the sync over.
fn skip(dir: &'static Path) {
    let mut st = STATE.lock().unwrap();
    if !st.dirty {
        let marker = dir.join(MARKER);
        if let Ok(f) = std::fs::File::create(&marker) {
            use std::os::fd::AsRawFd;
            real_fsync(f.as_raw_fd());
            if let Ok(d) = std::fs::File::open(dir) {
                real_fsync(d.as_raw_fd());
            }
        }
        st.dirty = true;
    }
    st.generation += 1;
    if !st.syncer {
        st.syncer = true;
        let _ = std::thread::Builder::new().name("qs-cachesync".into()).spawn(move || syncer(dir));
    }
    WAKE.notify_one();
}

fn syncer(dir: &'static Path) {
    let mut st = STATE.lock().unwrap();
    loop {
        while !st.dirty {
            st = WAKE.wait(st).unwrap();
        }
        // Wait until the writes stop, then flush them all with one syncfs.
        let mut seen = st.generation;
        loop {
            st = WAKE.wait_timeout(st, SETTLE).unwrap().0;
            if st.generation == seen {
                break;
            }
            seen = st.generation;
        }
        drop(st);
        if let Ok(d) = std::fs::File::open(dir) {
            use std::os::fd::AsRawFd;
            unsafe { libc::syncfs(d.as_raw_fd()) };
        }
        st = STATE.lock().unwrap();
        if st.generation == seen {
            let _ = std::fs::remove_file(dir.join(MARKER));
            st.dirty = false;
        }
    }
}

// Exported from the executable, so libQt6Core's calls bind here instead of libc's.
#[unsafe(no_mangle)]
pub extern "C" fn fsync(fd: c_int) -> c_int {
    match in_cache(fd) {
        Some(dir) => {
            skip(dir);
            0
        }
        None => real_fsync(fd),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn fdatasync(fd: c_int) -> c_int {
    match in_cache(fd) {
        Some(dir) => {
            skip(dir);
            0
        }
        None => real_fdatasync(fd),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;

    // One test: DIR is process-global.
    #[test]
    fn cache_flushes_are_deferred_and_a_dirty_cache_is_dropped() {
        let root = std::env::temp_dir().join(format!("qs-rs-cachesync-{}", std::process::id()));
        let dir = root.join("qmlcache");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stale.qmlc"), b"torn").unwrap();
        std::fs::write(dir.join(MARKER), b"").unwrap();
        init(dir.clone());
        assert!(!dir.join("stale.qmlc").exists() && !dir.join(MARKER).exists());

        let outside = std::fs::File::create(root.join("config.json")).unwrap();
        assert_eq!(fdatasync(outside.as_raw_fd()), 0);
        assert!(!dir.join(MARKER).exists(), "a flush outside the cache must not mark it");

        let cached = std::fs::File::create(dir.join("a.qmlc")).unwrap();
        assert_eq!(fdatasync(cached.as_raw_fd()), 0);
        assert!(dir.join(MARKER).exists(), "the marker is durable before the skipped flush returns");
        let t = std::time::Instant::now();
        while dir.join(MARKER).exists() && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!dir.join(MARKER).exists(), "the syncer clears the marker after syncfs");
        assert!(dir.join("a.qmlc").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_keeps_mapped_and_recent_entries() {
        let dir = std::env::temp_dir().join(format!("qs-rs-cacheprune-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let week_ago = std::time::SystemTime::now() - Duration::from_secs(8 * 24 * 3600);
        for name in ["old.qmlc", "mapped.qmlc", "recent.qmlc", "other.txt"] {
            std::fs::write(dir.join(name), b"unit").unwrap();
            if name != "recent.qmlc" {
                std::fs::File::open(dir.join(name)).unwrap().set_modified(week_ago).unwrap();
            }
        }
        let mapped = std::fs::File::open(dir.join("mapped.qmlc")).unwrap();
        let p = unsafe { libc::mmap(std::ptr::null_mut(), 4, libc::PROT_READ, libc::MAP_PRIVATE, mapped.as_raw_fd(), 0) };
        assert_ne!(p, libc::MAP_FAILED);
        prune(&dir, KEEP);
        assert!(!dir.join("old.qmlc").exists());
        assert!(dir.join("recent.qmlc").exists() && dir.join("other.txt").exists());
        let m = std::fs::metadata(dir.join("mapped.qmlc")).unwrap().modified().unwrap();
        assert!(m > week_ago + Duration::from_secs(24 * 3600), "a mapped entry is marked as used");
        unsafe { libc::munmap(p, 4) };
        let _ = std::fs::remove_dir_all(&dir);
    }
}
