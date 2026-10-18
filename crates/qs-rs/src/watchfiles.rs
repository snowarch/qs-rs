//! `Quickshell.watchFiles`: a change to a file the shell loaded reloads it, like upstream's
//! generation watcher. Two differences: the directories are watched, so an editor that saves by
//! replacing the file (a new inode) is seen at every save, not only the first; and a burst of
//! saves (a git checkout, a formatter over many files) becomes one reload, where upstream reloads
//! per file and a reload landing on another is how inir's shell used to crash.

use notify::event::{AccessKind, AccessMode, EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::Duration;

/// Quiet time after the last event before the reload: a save is one close or rename, a checkout
/// many within a few ms.
const SETTLE: Duration = Duration::from_millis(60);

struct State {
    watcher: RecommendedWatcher,
    dirs: BTreeSet<PathBuf>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
/// What the current generation was built from; read by the watcher and debounce threads.
static HASHES: Mutex<BTreeMap<PathBuf, u64>> = Mutex::new(BTreeMap::new());

/// Watches the files of a fresh scan (first load and every reload).
pub fn track(hashes: BTreeMap<PathBuf, u64>) {
    let dirs: BTreeSet<PathBuf> = hashes.keys().filter_map(|f| f.parent().map(PathBuf::from)).collect();
    *HASHES.lock().unwrap() = hashes;
    let mut state = STATE.lock().unwrap();
    if state.is_none() {
        let Some(watcher) = start() else { return };
        *state = Some(State { watcher, dirs: BTreeSet::new() });
    }
    let state = state.as_mut().unwrap();
    for gone in state.dirs.difference(&dirs) {
        let _ = state.watcher.unwatch(gone);
    }
    for new in dirs.difference(&state.dirs) {
        if let Err(e) = state.watcher.watch(new, RecursiveMode::NonRecursive) {
            quickshell::qs_log!(warn, "quickshell.qsrs", "watchFiles: cannot watch {}: {e}", new.display());
        }
    }
    state.dirs = dirs;
}

fn start() -> Option<RecommendedWatcher> {
    let (tx, rx) = mpsc::channel::<PathBuf>();
    std::thread::Builder::new()
        .name("qs-watchfiles".into())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut touched = BTreeSet::from([first]);
                loop {
                    match rx.recv_timeout(SETTLE) {
                        Ok(p) => {
                            touched.insert(p);
                        }
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                if quickshell::common::watch_files() && touched.iter().any(changed) {
                    quickshell::common::request_reload(false);
                }
            }
        })
        .ok()?;
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        // A finished write, a file renamed into place or created; plain modify events come
        // mid-write and the close follows them.
        let saved = matches!(
            event.kind,
            EventKind::Access(AccessKind::Close(AccessMode::Write)) | EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
        );
        if saved {
            forward(&tx, event.paths);
        }
    });
    match watcher {
        Ok(w) => Some(w),
        Err(e) => {
            quickshell::qs_log!(warn, "quickshell.qsrs", "watchFiles: no file watcher: {e}");
            None
        }
    }
}

fn forward(tx: &Sender<PathBuf>, paths: Vec<PathBuf>) {
    let hashes = HASHES.lock().unwrap();
    for p in paths.into_iter().filter(|p| hashes.contains_key(p)) {
        let _ = tx.send(p);
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
