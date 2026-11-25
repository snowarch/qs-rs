//! `Quickshell.watchFiles`: a change to a file the shell loaded reloads it, like upstream's
//! generation watcher. Shares the single process-wide inotify watcher in `quickshell_io::watch`
//! and spawns a short-lived debounce thread only when a watched file is actually saved (zero extra
//! idle threads at rest).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Quiet time after the last event before the reload: a save is one close or rename, a checkout
/// many within a few ms.
const SETTLE: Duration = Duration::from_millis(60);

static GROUP: Mutex<Option<quickshell_io::watch::DirGroup>> = Mutex::new(None);
/// What the current generation was built from; read by the watcher callback and debounce pass.
static HASHES: Mutex<BTreeMap<PathBuf, u64>> = Mutex::new(BTreeMap::new());

struct Pending {
    touched: BTreeSet<PathBuf>,
    deadline: Option<Instant>,
    running: bool,
}

static PENDING: Mutex<Pending> = Mutex::new(Pending { touched: BTreeSet::new(), deadline: None, running: false });
static PENDING_WAKE: Condvar = Condvar::new();

/// Watches the files of a fresh scan (first load and every reload).
/// When `Quickshell.watchFiles` is off, only the file hashes are kept and no watches are added.
pub fn track(hashes: BTreeMap<PathBuf, u64>) {
    *HASHES.lock().unwrap() = hashes;
    sync(quickshell::common::watch_files());
}

/// Called when `Quickshell.watchFiles` toggles: installs watches when turned on, drops them when off.
pub fn sync(enabled: bool) {
    let dirs: BTreeSet<PathBuf> = if enabled {
        HASHES.lock().unwrap().keys().filter_map(|f| f.parent().map(PathBuf::from)).collect()
    } else {
        BTreeSet::new()
    };
    let mut group = GROUP.lock().unwrap();
    if dirs.is_empty() {
        if let Some(g) = group.as_mut() {
            g.set_dirs(BTreeSet::new());
        }
        return;
    }
    if group.is_none() {
        let Some(g) = quickshell_io::watch::DirGroup::new(on_saved) else {
            quickshell::qs_log!(warn, "quickshell.qsrs", "watchFiles: no file watcher available");
            return;
        };
        *group = Some(g);
    }
    group.as_mut().unwrap().set_dirs(dirs);
}

fn on_saved(paths: &[PathBuf]) {
    let matched: Vec<PathBuf> = {
        let hashes = HASHES.lock().unwrap();
        paths.iter().filter(|p| hashes.contains_key(*p)).cloned().collect()
    };
    if matched.is_empty() {
        return;
    }
    let mut p = PENDING.lock().unwrap();
    p.touched.extend(matched);
    p.deadline = Some(Instant::now() + SETTLE);
    if !p.running {
        p.running = true;
        let _ = std::thread::Builder::new().name("qs-watchfiles".into()).spawn(debounce_loop);
    }
    PENDING_WAKE.notify_one();
}

fn debounce_loop() {
    loop {
        let touched = {
            let mut p = PENDING.lock().unwrap();
            loop {
                let Some(deadline) = p.deadline else {
                    p.running = false;
                    return;
                };
                let now = Instant::now();
                if now >= deadline {
                    p.deadline = None;
                    break std::mem::take(&mut p.touched);
                }
                p = PENDING_WAKE.wait_timeout(p, deadline - now).unwrap().0;
            }
        };
        if quickshell::common::watch_files() && touched.iter().any(changed) {
            quickshell::common::request_reload(false);
        }
        let mut p = PENDING.lock().unwrap();
        if p.touched.is_empty() && p.deadline.is_none() {
            p.running = false;
            return;
        }
    }
}

/// Upstream's checks: an empty file is the truncate half of a two-step save, and a save with the
/// same content is no change.
fn changed(path: &PathBuf) -> bool {
    let Ok(data) = std::fs::read(path) else { return false };
    if data.is_empty() {
        return false;
    }
    HASHES.lock().unwrap().get(path).is_some_and(|&h| h != crate::scan::content_hash(&data))
}
