//! Native Rust backend for `Qt.labs.folderlistmodel.FolderListModel`.
//!
//! Replaces Qt's per-instance `FileInfoThread` OS threads with the shared `crate::watch` inotify
//! thread and the shared bounded `crate::pool` worker pool (zero extra permanent threads).

#[cxx_qt::bridge]
pub mod ffi {
    struct FolderEntryData {
        file_name: String,
        file_path: String,
        file_base_name: String,
        file_suffix: String,
        file_size: i64,
        file_modified_ms: i64,
        file_accessed_ms: i64,
        file_is_dir: bool,
    }

    struct FolderScanRequest {
        folder: String,
        name_filters: Vec<String>,
        sort_field: i32,
        sort_reversed: bool,
        show_files: bool,
        show_dirs: bool,
        show_dirs_first: bool,
        show_dot: bool,
        show_dot_dot: bool,
        show_hidden: bool,
        show_only_readable: bool,
        case_sensitive: bool,
        sort_case_sensitive: bool,
    }

    extern "Rust" {
        fn folder_watch_register(dir_cb: usize, scan_cb: usize) -> u64;
        fn folder_watch_set(id: u64, dir: &str);
        fn folder_watch_unregister(id: u64);
        fn folder_scan_async(id: u64, gen_id: u64, req: FolderScanRequest);
        fn folder_take_scan(id: u64, gen_id: u64) -> Vec<FolderEntryData>;
    }
}

use ffi::{FolderEntryData, FolderScanRequest};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

type DirNotifyFn = extern "C" fn(u64);
type ScanDoneFn = extern "C" fn(u64, u64);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static DIR_NOTIFY_CB: AtomicUsize = AtomicUsize::new(0);
static SCAN_DONE_CB: AtomicUsize = AtomicUsize::new(0);

struct ScanStore {
    /// Highest requested generation per model id so an out-of-order older worker never overwrites a newer scan.
    latest_gen: BTreeMap<u64, u64>,
    /// Completed async scans waiting for the Qt GUI thread to pick them up.
    results: BTreeMap<(u64, u64), Vec<FolderEntryData>>,
}

static SCANS: Mutex<ScanStore> = Mutex::new(ScanStore { latest_gen: BTreeMap::new(), results: BTreeMap::new() });

struct WatchHub {
    /// Active watches by model id -> directory path (read from watcher thread).
    targets: Arc<Mutex<HashMap<u64, PathBuf>>>,
    group: Option<crate::watch::DirGroup>,
}

fn watch_hub() -> &'static Mutex<WatchHub> {
    static HUB: OnceLock<Mutex<WatchHub>> = OnceLock::new();
    HUB.get_or_init(|| {
        let targets: Arc<Mutex<HashMap<u64, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
        let cb_targets = Arc::clone(&targets);
        let group = crate::watch::DirGroup::new_changes(move |paths| {
            let hit_ids: Vec<u64> = {
                let map = cb_targets.lock().unwrap();
                map.iter()
                    .filter(|(_, dir)| paths.iter().any(|p| p == *dir || p.parent() == Some(dir.as_path())))
                    .map(|(&id, _)| id)
                    .collect()
            };
            let raw = DIR_NOTIFY_CB.load(AtomicOrdering::Acquire);
            if raw == 0 || hit_ids.is_empty() {
                return;
            }
            // SAFETY: registered by C++ `qsrs_folderlist_dir_changed`, an `extern "C" fn(u64)`.
            let cb: DirNotifyFn = unsafe { std::mem::transmute(raw) };
            for id in hit_ids {
                cb(id);
            }
        });
        Mutex::new(WatchHub { targets, group })
    })
}

fn sync_watched_dirs(hub: &mut WatchHub) {
    let dirs: BTreeSet<PathBuf> = hub.targets.lock().unwrap().values().cloned().collect();
    if let Some(group) = hub.group.as_mut() {
        group.set_dirs(dirs);
    }
}

fn folder_watch_register(dir_cb: usize, scan_cb: usize) -> u64 {
    if dir_cb != 0 {
        DIR_NOTIFY_CB.store(dir_cb, AtomicOrdering::Release);
    }
    if scan_cb != 0 {
        SCAN_DONE_CB.store(scan_cb, AtomicOrdering::Release);
    }
    NEXT_ID.fetch_add(1, AtomicOrdering::Relaxed)
}

fn folder_watch_set(id: u64, dir: &str) {
    let mut hub = watch_hub().lock().unwrap();
    {
        let mut map = hub.targets.lock().unwrap();
        if dir.is_empty() {
            map.remove(&id);
        } else {
            map.insert(id, PathBuf::from(dir));
        }
    }
    sync_watched_dirs(&mut hub);
}

fn folder_watch_unregister(id: u64) {
    {
        let mut hub = watch_hub().lock().unwrap();
        hub.targets.lock().unwrap().remove(&id);
        sync_watched_dirs(&mut hub);
    }
    let mut s = SCANS.lock().unwrap();
    s.latest_gen.remove(&id);
    s.results.retain(|&(mid, _), _| mid != id);
}

fn folder_scan_async(id: u64, gen_id: u64, req: FolderScanRequest) {
    SCANS.lock().unwrap().latest_gen.insert(id, gen_id);
    crate::pool::submit(move || {
        // Skip work if a newer generation superseded this request while queued.
        if SCANS.lock().unwrap().latest_gen.get(&id).copied() != Some(gen_id) {
            return;
        }
        let entries = scan_folder(&req);
        {
            let mut s = SCANS.lock().unwrap();
            if s.latest_gen.get(&id).copied() != Some(gen_id) {
                return;
            }
            s.results.retain(|&(mid, g), _| mid != id || g > gen_id);
            s.results.insert((id, gen_id), entries);
        }
        let raw = SCAN_DONE_CB.load(AtomicOrdering::Acquire);
        if raw != 0 {
            // SAFETY: registered by C++ `qsrs_folderlist_scan_done`, an `extern "C" fn(u64, u64)`.
            let cb: ScanDoneFn = unsafe { std::mem::transmute(raw) };
            cb(id, gen_id);
        }
    });
}

fn folder_take_scan(id: u64, gen_id: u64) -> Vec<FolderEntryData> {
    SCANS.lock().unwrap().results.remove(&(id, gen_id)).unwrap_or_default()
}

/// Splits `name` into `(baseName, completeSuffix)` matching Qt's `QFileInfo::baseName()` and
/// `QFileInfo::completeSuffix()` as used by `QQuickFolderListModel`.
fn split_base_and_suffix(name: &str) -> (String, String) {
    if name == "." || name == ".." {
        return (String::new(), String::new());
    }
    match name.find('.') {
        Some(i) => (name[..i].to_string(), name[i + 1..].to_string()),
        None => (name.to_string(), String::new()),
    }
}

fn system_time_ms(t: std::io::Result<SystemTime>) -> (i64, i128) {
    match t {
        Ok(st) => match st.duration_since(UNIX_EPOCH) {
            Ok(d) => (d.as_millis() as i64, d.as_nanos() as i128),
            Err(e) => {
                let d = e.duration();
                (-(d.as_millis() as i64), -(d.as_nanos() as i128))
            }
        },
        Err(_) => (0, 0),
    }
}

fn is_readable(path: &Path) -> bool {
    let Ok(cpath) = CString::new(path.as_os_str().as_bytes()) else { return false };
    unsafe { libc::access(cpath.as_ptr(), libc::R_OK) == 0 }
}

/// Wildcard glob matcher (`*`, `?`, `[abc]`/`[a-z]`/`[!a-z]`, `\`), matching QDir wildcard rules.
pub fn glob_match(pattern: &str, text: &str, case_sensitive: bool) -> bool {
    if pattern == "*" {
        return true;
    }
    let pat: Vec<char> = if case_sensitive { pattern.chars().collect() } else { pattern.to_lowercase().chars().collect() };
    let txt: Vec<char> = if case_sensitive { text.chars().collect() } else { text.to_lowercase().chars().collect() };
    glob_match_slice(&pat, &txt)
}

fn glob_match_slice(pat: &[char], txt: &[char]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_p, mut star_t) = (None::<usize>, 0usize);

    while ti < txt.len() {
        if pi < pat.len() && pat[pi] == '*' {
            star_p = Some(pi);
            pi += 1;
            star_t = ti;
        } else if pi < pat.len() && pat[pi] == '?' {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == '[' {
            if let Some((matched, next_p)) = match_char_class(&pat[pi..], txt[ti]) {
                if matched {
                    pi += next_p;
                    ti += 1;
                    continue;
                }
            } else if pat[pi] == txt[ti] {
                pi += 1;
                ti += 1;
                continue;
            }
            if let Some(sp) = star_p {
                pi = sp + 1;
                star_t += 1;
                ti = star_t;
            } else {
                return false;
            }
        } else if pi < pat.len() && pat[pi] == '\\' && pi + 1 < pat.len() {
            if pat[pi + 1] == txt[ti] {
                pi += 2;
                ti += 1;
            } else if let Some(sp) = star_p {
                pi = sp + 1;
                star_t += 1;
                ti = star_t;
            } else {
                return false;
            }
        } else if pi < pat.len() && pat[pi] == txt[ti] {
            pi += 1;
            ti += 1;
        } else if let Some(sp) = star_p {
            pi = sp + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }

    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }
    pi == pat.len()
}

fn match_char_class(pat: &[char], ch: char) -> Option<(bool, usize)> {
    // pat[0] == '['
    let mut i = 1;
    if i >= pat.len() {
        return None;
    }
    let negate = pat[i] == '!' || pat[i] == '^';
    if negate {
        i += 1;
    }
    let start = i;
    let mut matched = false;
    while i < pat.len() {
        if pat[i] == ']' && i > start {
            return Some((if negate { !matched } else { matched }, i + 1));
        }
        let c = pat[i];
        if i + 2 < pat.len() && pat[i + 1] == '-' && pat[i + 2] != ']' {
            let end = pat[i + 2];
            if c <= ch && ch <= end {
                matched = true;
            }
            i += 3;
        } else {
            if c == ch {
                matched = true;
            }
            i += 1;
        }
    }
    None
}

struct SortItem {
    entry: FolderEntryData,
    modified_ns: i128,
    sort_name: String,
    sort_suffix: String,
}

fn make_entry(name: String, path: &Path, meta: &std::fs::Metadata, sort_case_sensitive: bool) -> SortItem {
    let is_dir = meta.is_dir();
    let (file_base_name, file_suffix) = split_base_and_suffix(&name);
    let (file_modified_ms, modified_ns) = system_time_ms(meta.modified());
    let (file_accessed_ms, _) = system_time_ms(meta.accessed());
    let sort_name = if sort_case_sensitive { String::new() } else { name.to_lowercase() };
    let sort_suffix = if sort_case_sensitive { String::new() } else { file_suffix.to_lowercase() };
    SortItem {
        entry: FolderEntryData {
            file_name: name,
            file_path: path.to_string_lossy().into_owned(),
            file_base_name,
            file_suffix,
            file_size: meta.len() as i64,
            file_modified_ms,
            file_accessed_ms,
            file_is_dir: is_dir,
        },
        modified_ns,
        sort_name,
        sort_suffix,
    }
}

pub fn scan_folder(req: &FolderScanRequest) -> Vec<FolderEntryData> {
    let dir = Path::new(&req.folder);
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let match_all = req.name_filters.is_empty() || req.name_filters.iter().any(|f| f == "*");
    let mut items: Vec<SortItem> = Vec::new();

    if req.show_dirs {
        if req.show_dot
            && let Ok(meta) = std::fs::metadata(dir)
            && (!req.show_only_readable || is_readable(dir))
        {
            items.push(make_entry(".".into(), &dir.join("."), &meta, req.sort_case_sensitive));
        }
        if req.show_dot_dot {
            let parent = dir.join("..");
            if let Ok(meta) = std::fs::metadata(&parent)
                && (!req.show_only_readable || is_readable(&parent))
            {
                items.push(make_entry("..".into(), &parent, &meta, req.sort_case_sensitive));
            }
        }
    }

    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !req.show_hidden && name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        // Follow symlinks like QFileInfo::isDir() / QDir does; fall back to symlink_metadata for broken links.
        let meta = match std::fs::metadata(&path).or_else(|_| std::fs::symlink_metadata(&path)) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let is_dir = meta.is_dir();
        if is_dir && !req.show_dirs {
            continue;
        }
        if !is_dir && !req.show_files {
            continue;
        }
        // Qt's QDir::AllDirs keeps directories regardless of `nameFilters`.
        if !is_dir && !match_all && !req.name_filters.iter().any(|pat| glob_match(pat, &name, req.case_sensitive)) {
            continue;
        }
        if req.show_only_readable && !is_readable(&path) {
            continue;
        }
        items.push(make_entry(name, &path, &meta, req.sort_case_sensitive));
    }

    let cmp_name = |a: &SortItem, b: &SortItem| -> Ordering {
        if req.sort_case_sensitive {
            a.entry.file_name.cmp(&b.entry.file_name)
        } else {
            a.sort_name.cmp(&b.sort_name).then_with(|| a.entry.file_name.cmp(&b.entry.file_name))
        }
    };

    let cmp_primary = |a: &SortItem, b: &SortItem| -> Ordering {
        let ord = match req.sort_field {
            0 => Ordering::Equal, // Unsorted
            1 => cmp_name(a, b),  // Name
            2 => b.modified_ns.cmp(&a.modified_ns).then_with(|| cmp_name(a, b)), // Time (newest first)
            3 => b.entry.file_size.cmp(&a.entry.file_size).then_with(|| cmp_name(a, b)), // Size (largest first)
            4 => {
                // Type (by suffix)
                let s = if req.sort_case_sensitive {
                    a.entry.file_suffix.cmp(&b.entry.file_suffix)
                } else {
                    a.sort_suffix.cmp(&b.sort_suffix).then_with(|| a.entry.file_suffix.cmp(&b.entry.file_suffix))
                };
                s.then_with(|| cmp_name(a, b))
            }
            _ => cmp_name(a, b),
        };
        if req.sort_reversed { ord.reverse() } else { ord }
    };

    if req.sort_field != 0 || req.show_dirs_first {
        items.sort_by(|a, b| {
            // `showDirsFirst` always keeps directories before files, even when `sortReversed` is true.
            if req.show_dirs_first && a.entry.file_is_dir != b.entry.file_is_dir {
                return b.entry.file_is_dir.cmp(&a.entry.file_is_dir);
            }
            cmp_primary(a, b)
        });
    } else if req.sort_reversed {
        items.reverse();
    }

    items.into_iter().map(|i| i.entry).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    fn base_req(dir: &Path) -> FolderScanRequest {
        FolderScanRequest {
            folder: dir.to_string_lossy().into_owned(),
            name_filters: vec!["*".into()],
            sort_field: 1,
            sort_reversed: false,
            show_files: true,
            show_dirs: true,
            show_dirs_first: false,
            show_dot: false,
            show_dot_dot: false,
            show_hidden: false,
            show_only_readable: false,
            case_sensitive: true,
            sort_case_sensitive: true,
        }
    }

    #[test]
    fn glob_patterns_match_like_qdir() {
        assert!(glob_match("*.png", "wall.png", true));
        assert!(!glob_match("*.png", "wall.PNG", true));
        assert!(glob_match("*.png", "wall.PNG", false));
        assert!(glob_match("*foo*bar*.jpg", "my_foo_123_bar_x.jpg", false));
        assert!(glob_match("img_?.webp", "img_4.webp", true));
        assert!(!glob_match("img_?.webp", "img_42.webp", true));
        assert!(glob_match("[a-c]*.txt", "beta.txt", true));
        assert!(!glob_match("[!a-c]*.txt", "beta.txt", true));
    }

    #[test]
    fn scan_sorts_filters_and_matches_qt_semantics() {
        let dir = std::env::temp_dir().join(format!("qsrs-flm-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub_b")).unwrap();
        std::fs::create_dir_all(dir.join("sub_a")).unwrap();
        std::fs::write(dir.join("big.txt"), "12345").unwrap();
        std::fs::write(dir.join("a.b.txt"), "1").unwrap();
        std::fs::write(dir.join("other.md"), "123").unwrap();
        std::fs::write(dir.join(".hidden.txt"), "12").unwrap();

        // Name + showDirsFirst + sortReversed: dirs stay first, both groups reversed.
        let mut req = base_req(&dir);
        req.name_filters = vec!["*.txt".into()];
        req.show_dirs_first = true;
        req.sort_reversed = true;
        let out = scan_folder(&req);
        let names: Vec<&str> = out.iter().map(|e| e.file_name.as_str()).collect();
        assert_eq!(names, vec!["sub_b", "sub_a", "big.txt", "a.b.txt"]);
        assert_eq!(out[3].file_base_name, "a");
        assert_eq!(out[3].file_suffix, "b.txt");

        // Size sort: largest first, files only.
        let mut req = base_req(&dir);
        req.show_dirs = false;
        req.sort_field = 3;
        let out = scan_folder(&req);
        let names: Vec<&str> = out.iter().map(|e| e.file_name.as_str()).collect();
        assert_eq!(names, vec!["big.txt", "other.md", "a.b.txt"]);

        // Hidden files included only when requested.
        req.show_hidden = true;
        let out = scan_folder(&req);
        assert!(out.iter().any(|e| e.file_name == ".hidden.txt"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    static TEST_DIR_HITS: AtomicUsize = AtomicUsize::new(0);
    extern "C" fn on_test_dir(_id: u64) {
        TEST_DIR_HITS.fetch_add(1, AtomicOrdering::SeqCst);
    }

    #[test]
    fn shared_watcher_notifies_on_file_changes() {
        let dir = std::env::temp_dir().join(format!("qsrs-flm-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        TEST_DIR_HITS.store(0, AtomicOrdering::SeqCst);
        let id = folder_watch_register(on_test_dir as *const () as usize, 0);
        folder_watch_set(id, &dir.to_string_lossy());

        std::fs::write(dir.join("new.png"), "x").unwrap();
        let start = Instant::now();
        while TEST_DIR_HITS.load(AtomicOrdering::SeqCst) == 0 && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(TEST_DIR_HITS.load(AtomicOrdering::SeqCst) >= 1);

        folder_watch_unregister(id);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
