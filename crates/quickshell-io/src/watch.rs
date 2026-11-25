//! One inotify watcher (one thread) for the whole process. A subscriber names a file; the hub
//! watches its parent directory (so creation and atomic replacement are seen too) and calls the
//! subscriber for every non-access event that names the file.

use notify::event::{AccessKind, AccessMode, EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

type Callback = Box<dyn Fn() + Send>;
type SaveCallback = Box<dyn Fn(&[PathBuf]) + Send>;

/// Subscribers, read by the watcher thread. Never held while calling into the watcher: its
/// `watch`/`unwatch` wait for that same thread.
static SUBS: Mutex<Vec<(u64, PathBuf, Callback)>> = Mutex::new(Vec::new());
/// Whole-directory save subscribers (used by `Quickshell.watchFiles`), sharing the same watcher.
static SAVE_SUBS: Mutex<Vec<(u64, SaveCallback)>> = Mutex::new(Vec::new());

struct Dirs {
    watcher: RecommendedWatcher,
    count: HashMap<PathBuf, usize>,
    next: u64,
}

fn dirs() -> Option<&'static Mutex<Dirs>> {
    static DIRS: OnceLock<Option<Mutex<Dirs>>> = OnceLock::new();
    DIRS.get_or_init(|| {
        let watcher = notify::recommended_watcher(|res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            let saved = matches!(
                event.kind,
                EventKind::Access(AccessKind::Close(AccessMode::Write)) | EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
            );
            if saved {
                for (_, cb) in SAVE_SUBS.lock().unwrap().iter() {
                    cb(&event.paths);
                }
            }
            if event.kind.is_access() {
                return;
            }
            for (_, file, cb) in SUBS.lock().unwrap().iter() {
                if event.paths.iter().any(|p| p == file) {
                    cb();
                }
            }
        });
        watcher.ok().map(|watcher| Mutex::new(Dirs { watcher, count: HashMap::new(), next: 0 }))
    })
    .as_ref()
}

fn add_dir(d: &mut Dirs, dir: &Path) -> bool {
    let n = d.count.entry(dir.to_path_buf()).or_insert(0);
    *n += 1;
    if *n == 1 && let Err(e) = d.watcher.watch(dir, RecursiveMode::NonRecursive) {
        d.count.remove(dir);
        quickshell::qs_log!(warn, "quickshell.io.watch", "cannot watch {}: {e}", dir.display());
        return false;
    }
    true
}

fn remove_dir(d: &mut Dirs, dir: &Path) {
    if let Some(n) = d.count.get_mut(dir) {
        *n -= 1;
        if *n == 0 {
            d.count.remove(dir);
            let _ = d.watcher.unwatch(dir);
        }
    }
}

/// Keeps a subscription alive; dropping it unsubscribes.
pub struct Subscription {
    id: u64,
    dir: PathBuf,
}

/// Calls `cb` on the watcher thread whenever `file` changes, is created, removed or replaced.
pub fn subscribe(file: &Path, cb: impl Fn() + Send + 'static) -> Option<Subscription> {
    let dir = file.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    let mut d = dirs()?.lock().unwrap();
    let id = d.next;
    d.next += 1;
    if !add_dir(&mut d, &dir) {
        return None;
    }
    SUBS.lock().unwrap().push((id, file.to_path_buf(), Box::new(cb)));
    Some(Subscription { id, dir })
}

impl Drop for Subscription {
    fn drop(&mut self) {
        // Out of SUBS first: no callback runs for this subscriber after drop returns.
        SUBS.lock().unwrap().retain(|(id, ..)| *id != self.id);
        let Some(dirs) = dirs() else { return };
        let mut d = dirs.lock().unwrap();
        remove_dir(&mut d, &self.dir);
    }
}

/// Keeps a dynamic set of directories watched on the shared inotify thread, receiving finished
/// write / create / rename events for paths inside them.
pub struct DirGroup {
    id: u64,
    dirs: BTreeSet<PathBuf>,
}

impl DirGroup {
    pub fn new(cb: impl Fn(&[PathBuf]) + Send + 'static) -> Option<Self> {
        let mut d = dirs()?.lock().unwrap();
        let id = d.next;
        d.next += 1;
        SAVE_SUBS.lock().unwrap().push((id, Box::new(cb)));
        Some(Self { id, dirs: BTreeSet::new() })
    }

    pub fn set_dirs(&mut self, next: BTreeSet<PathBuf>) {
        let Some(dirs) = dirs() else { return };
        let mut d = dirs.lock().unwrap();
        for gone in self.dirs.difference(&next) {
            remove_dir(&mut d, gone);
        }
        let mut active = BTreeSet::new();
        for keep in self.dirs.intersection(&next) {
            active.insert(keep.clone());
        }
        for new in next.difference(&self.dirs) {
            if add_dir(&mut d, new) {
                active.insert(new.clone());
            }
        }
        self.dirs = active;
    }
}

impl Drop for DirGroup {
    fn drop(&mut self) {
        SAVE_SUBS.lock().unwrap().retain(|(id, _)| *id != self.id);
        let Some(dirs) = dirs() else { return };
        let mut d = dirs.lock().unwrap();
        for gone in std::mem::take(&mut self.dirs) {
            remove_dir(&mut d, &gone);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    fn wait_for(n: &AtomicUsize, min: usize) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            if n.load(Ordering::SeqCst) >= min {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn one_thread_many_files() {
        let dir = std::env::temp_dir().join(format!("qsrs-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("b"));
        let (na, nb) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (ca, cb) = (na.clone(), nb.clone());
        let sa = subscribe(&a, move || { ca.fetch_add(1, Ordering::SeqCst); }).unwrap();
        let sb = subscribe(&b, move || { cb.fetch_add(1, Ordering::SeqCst); }).unwrap();
        // Creation and atomic replacement are seen; the other file's subscriber stays quiet.
        std::fs::write(&a, "1").unwrap();
        assert!(wait_for(&na, 1));
        std::fs::write(dir.join("tmp"), "2").unwrap();
        let before = na.load(Ordering::SeqCst);
        std::fs::rename(dir.join("tmp"), &a).unwrap();
        assert!(wait_for(&na, before + 1));
        assert_eq!(nb.load(Ordering::SeqCst), 0);
        // Dropped subscribers get nothing more; the dir stays watched while one remains.
        drop(sa);
        let after = na.load(Ordering::SeqCst);
        std::fs::write(&b, "3").unwrap();
        assert!(wait_for(&nb, 1));
        std::fs::write(&a, "4").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(na.load(Ordering::SeqCst), after);
        drop(sb);
        assert!(dirs().unwrap().lock().unwrap().count.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
