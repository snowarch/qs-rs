//! Desktop entry scanning and parsing (upstream core/desktopentry.cpp). The QObjects that QML sees
//! (DesktopEntries, DesktopEntry, DesktopAction) are C++ in include/desktopentry.h and get their
//! data from `desktop_scan`.

#[cxx_qt::bridge]
pub mod ffi {
    struct DesktopActionData {
        id: String,
        name: String,
        icon: String,
        exec_string: String,
        command: Vec<String>,
    }

    struct DesktopEntryData {
        id: String,
        name: String,
        generic_name: String,
        startup_class: String,
        no_display: bool,
        hidden: bool,
        comment: String,
        icon: String,
        exec_string: String,
        command: Vec<String>,
        working_directory: String,
        terminal: bool,
        categories: Vec<String>,
        keywords: Vec<String>,
        actions: Vec<DesktopActionData>,
    }

    extern "Rust" {
        /// Every entry of every desktop path, lowest priority first (a later entry with the same
        /// id overrides an earlier one).
        fn desktop_scan() -> Vec<DesktopEntryData>;
        /// The application directories to watch, highest priority first.
        fn desktop_paths() -> Vec<String>;
        fn desktop_exec(command: &Vec<String>, working_directory: &str);
        fn desktop_watch_start(notify_cb: usize);
        fn desktop_watch_refresh();
    }
}

use ffi::{DesktopActionData, DesktopEntryData};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Default, Clone, Copy, PartialEq, Debug)]
struct LocaleRef<'a> {
    language: &'a str,
    territory: &'a str,
    modifier: &'a str,
}

impl<'a> LocaleRef<'a> {
    /// `lang_COUNTRY.ENCODING@MODIFIER` without allocating.
    #[inline]
    fn parse(s: &'a str) -> Self {
        let mut end = s.len();
        let mut modifier = "";
        if let Some(i) = s.find('@') {
            modifier = &s[i + 1..];
            end = i;
        }
        if let Some(i) = s[..end].find('.') {
            end = i;
        }
        let mut territory = "";
        if let Some(i) = s[..end].find('_') {
            territory = &s[i + 1..end];
            end = i;
        }
        LocaleRef { language: &s[..end], territory, modifier }
    }

    #[inline]
    fn is_valid(&self) -> bool {
        !self.language.is_empty()
    }
}

#[derive(Default, Clone, PartialEq, Debug)]
struct Locale {
    language: String,
    territory: String,
    modifier: String,
}

impl Locale {
    /// `lang_COUNTRY.ENCODING@MODIFIER`
    fn parse(s: &str) -> Self {
        let r = LocaleRef::parse(s);
        Locale {
            language: r.language.to_string(),
            territory: r.territory.to_string(),
            modifier: r.modifier.to_string(),
        }
    }

    #[inline]
    fn match_score(&self, other: &LocaleRef<'_>) -> i32 {
        if self.language != other.language
            || (!other.modifier.is_empty() && self.modifier != other.modifier)
            || (!other.territory.is_empty() && self.territory != other.territory)
        {
            return 0;
        }
        1 + if other.territory.is_empty() { 0 } else { 2 } + if other.modifier.is_empty() { 0 } else { 1 }
    }

    fn system() -> &'static Locale {
        static LOCALE: OnceLock<Locale> = OnceLock::new();
        LOCALE.get_or_init(|| {
            let s = std::env::var("LC_MESSAGES").ok().filter(|s| !s.is_empty()).or_else(|| std::env::var("LANG").ok()).unwrap_or_default();
            Locale::parse(&s)
        })
    }
}

#[inline]
fn relevant_key(group: &str, key: &str) -> bool {
    if group == "Desktop Entry" {
        matches!(
            key,
            "Type"
                | "Name"
                | "GenericName"
                | "StartupWMClass"
                | "NoDisplay"
                | "Hidden"
                | "Comment"
                | "Icon"
                | "Exec"
                | "Path"
                | "Terminal"
                | "Categories"
                | "Keywords"
                | "Actions"
        )
    } else if group.starts_with("Desktop Action ") {
        matches!(key, "Name" | "Icon" | "Exec")
    } else {
        false
    }
}

fn split_list(value: &str) -> Vec<String> {
    value.split(';').filter(|s| !s.is_empty()).map(String::from).collect()
}

pub fn parse_text<'a>(id: &str, text: &'a str) -> DesktopEntryData {
    let system = Locale::system();
    let mut data = DesktopEntryData {
        id: id.to_string(),
        name: String::new(),
        generic_name: String::new(),
        startup_class: String::new(),
        no_display: false,
        hidden: false,
        comment: String::new(),
        icon: String::new(),
        exec_string: String::new(),
        command: Vec::new(),
        working_directory: String::new(),
        terminal: false,
        categories: Vec::new(),
        keywords: Vec::new(),
        actions: Vec::new(),
    };
    let mut group: &'a str = "";
    // Insertion order kept so later keys win the same way upstream's QHash replacement does.
    // All slices borrow directly from `text`: zero per-line String allocations.
    let mut entries: Vec<(&'a str, i32, bool, &'a str)> = Vec::with_capacity(16);
    let mut action_order: Vec<&'a str> = Vec::new();
    let mut pending: HashMap<&'a str, DesktopActionData> = HashMap::new();

    let mut finish = |group: &'a str, entries: &mut Vec<(&'a str, i32, bool, &'a str)>, data: &mut DesktopEntryData| {
        if group == "Desktop Entry" {
            let ty = entries.iter().find(|(k, ..)| *k == "Type").map(|(.., v)| *v);
            // Upstream returns before clearing here, so these keys leak into the next group.
            if ty != Some("Application") {
                return;
            }
            for &(key, _, _, value) in entries.iter() {
                match key {
                    "Name" => data.name = value.to_string(),
                    "GenericName" => data.generic_name = value.to_string(),
                    "StartupWMClass" => data.startup_class = value.to_string(),
                    "NoDisplay" => data.no_display = value == "true",
                    "Hidden" => data.hidden = value == "true",
                    "Comment" => data.comment = value.to_string(),
                    "Icon" => data.icon = value.to_string(),
                    "Exec" => {
                        data.command = parse_exec_string(value);
                        data.exec_string = value.to_string();
                    }
                    "Path" => data.working_directory = value.to_string(),
                    "Terminal" => data.terminal = value == "true",
                    "Categories" => data.categories = split_list(value),
                    "Keywords" => data.keywords = split_list(value),
                    "Actions" => action_order = value.split(';').filter(|s| !s.is_empty()).collect(),
                    _ => {}
                }
            }
        } else if let Some(name) = group.strip_prefix("Desktop Action ") {
            let mut action = DesktopActionData { id: name.to_string(), name: String::new(), icon: String::new(), exec_string: String::new(), command: Vec::new() };
            for &(key, _, _, value) in entries.iter() {
                match key {
                    "Name" => action.name = value.to_string(),
                    "Icon" => action.icon = value.to_string(),
                    "Exec" => {
                        action.command = parse_exec_string(value);
                        action.exec_string = value.to_string();
                    }
                    _ => {}
                }
            }
            pending.insert(name, action);
        }
        entries.clear();
    };

    // Trimmed: a CRLF file or a space after `[Desktop Entry]` lost the group, so the whole entry (upstream 30a5d60).
    for line in text.split('\n').map(str::trim).filter(|l| !l.is_empty()) {
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
            finish(group, &mut entries, &mut data);
            group = &line[1..line.len() - 1];
            continue;
        }
        let Some(split) = line.find('=') else {
            continue;
        };
        let (mut key, value) = (&line[..split], &line[split + 1..]);
        let mut locale = LocaleRef::default();
        if let Some(i) = key.find('[')
            && i != key.len() - 1
        {
            let end = if key.ends_with(']') { key.len() - 1 } else { key.len() };
            locale = LocaleRef::parse(&key[i + 1..end.max(i + 1)]);
            key = &key[..i];
        }
        if !relevant_key(group, key) {
            continue;
        }
        let new_score = if locale.is_valid() { system.match_score(&locale) } else { 0 };
        if locale.is_valid() && new_score == 0 {
            continue;
        }
        match entries.iter_mut().find(|(k, ..)| *k == key) {
            Some(old) => {
                if new_score > old.1 || (old.1 == 0 && !locale.is_valid()) {
                    old.1 = new_score;
                    old.2 = locale.is_valid();
                    old.3 = value;
                }
            }
            None => entries.push((key, new_score, locale.is_valid(), value)),
        }
    }
    finish(group, &mut entries, &mut data);
    drop(finish);
    for id in action_order {
        if let Some(a) = pending.remove(id) {
            data.actions.push(a);
        }
    }
    data
}

/// Splits an Exec value into arguments, dropping field codes (upstream ignores them too).
pub fn parse_exec_string(exec: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let (mut in_string, mut escape, mut percent) = (false, 0, false);
    for c in exec.chars() {
        if escape == 0 && c == '\\' {
            escape = 1;
        } else if in_string {
            if c == '\\' {
                escape += 1;
                if escape == 4 {
                    cur.push('\\');
                    escape = 0;
                }
            } else if escape == 2 {
                cur.push(c);
                escape = 0;
            } else if escape != 0 {
                cur.push(match c {
                    's' => ' ',
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    c => c,
                });
                escape = 0;
            } else if c == '"' || c == '\'' {
                in_string = false;
            } else {
                cur.push(c);
            }
        } else if escape != 0 {
            cur.push(c);
            escape = 0;
        } else if percent {
            if c == '%' {
                cur.push('%');
            }
            percent = false;
        } else if c == '%' {
            percent = true;
        } else if c == '"' || c == '\'' {
            in_string = true;
        } else if c == ' ' {
            if !cur.is_empty() {
                args.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        args.push(cur);
    }
    args
}

pub fn desktop_paths() -> Vec<String> {
    let mut paths = Vec::new();
    let mut home = std::env::var("XDG_DATA_HOME").unwrap_or_default();
    if home.is_empty()
        && let Ok(h) = std::env::var("HOME") {
            home = format!("{h}/.local/share");
        }
    if !home.is_empty() {
        paths.push(format!("{home}/applications"));
    }
    let dirs = std::env::var("XDG_DATA_DIRS").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    paths.extend(dirs.split(':').filter(|d| !d.is_empty()).map(|d| format!("{d}/applications")));
    paths
}

fn scan_dir(dir: &Path, prefix: &str, out: &mut Vec<DesktopEntryData>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<(String, String, bool)> = Vec::new();
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let is_dir = match e.file_type() {
            Ok(ft) if ft.is_dir() => true,
            Ok(ft) if ft.is_file() => {
                if !name.ends_with(".desktop") {
                    continue;
                }
                false
            }
            _ => {
                let Ok(meta) = std::fs::metadata(e.path()) else { continue };
                if meta.is_dir() {
                    true
                } else if meta.is_file() && name.ends_with(".desktop") {
                    false
                } else {
                    continue;
                }
            }
        };
        let lower = name.to_lowercase();
        items.push((lower, name, is_dir));
    }
    // QDir's default order: by name, case-insensitive.
    items.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    for (_, name, is_dir) in items {
        let path = dir.join(&name);
        if is_dir {
            let sub = if prefix.is_empty() { name.clone() } else { format!("{prefix}-{name}") };
            scan_dir(&path, &sub, out);
        } else {
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let base = &name[..name.rfind('.').unwrap_or(name.len())];
            let id = if prefix.is_empty() { base.to_string() } else { format!("{prefix}-{base}") };
            out.push(parse_text(&id, &String::from_utf8_lossy(&bytes)));
        }
    }
}

pub fn desktop_scan() -> Vec<DesktopEntryData> {
    let mut out = Vec::new();
    for path in desktop_paths().iter().rev() {
        let p = Path::new(path);
        if p.is_dir() {
            scan_dir(p, "", &mut out);
        }
    }
    out
}

fn desktop_exec(command: &Vec<String>, working_directory: &str) {
    if command.is_empty() {
        crate::qs_log!(warn, "quickshell.desktopentry", "Cannot start process as command is empty.");
        return;
    }
    crate::proc::spawn_detached(&crate::proc::Context {
        command: command.clone(),
        working_directory: working_directory.to_string(),
        environment: Vec::new(),
        clear_environment: false,
    });
}

#[derive(Default)]
struct WatchFilter {
    /// Existing `applications` directories and their immediate subdirectories.
    active_dirs: BTreeSet<PathBuf>,
    /// Next missing path component under an existing ancestor (so creating `applications/`
    /// triggers a rescan without reacting to unrelated files in `$HOME` or `/usr/share`).
    pending_targets: BTreeSet<PathBuf>,
}

impl WatchFilter {
    fn matches(&self, paths: &[PathBuf]) -> bool {
        paths.iter().any(|p| {
            self.active_dirs.contains(p)
                || p.parent().is_some_and(|parent| self.active_dirs.contains(parent))
                || self.pending_targets.contains(p)
        })
    }
}

struct DesktopWatch {
    roots: Vec<PathBuf>,
    filter: Arc<Mutex<WatchFilter>>,
    group: crate::watch::DirGroup,
}

impl DesktopWatch {
    fn new(roots: Vec<PathBuf>, on_change: impl Fn() + Send + 'static) -> Option<Self> {
        let filter = Arc::new(Mutex::new(WatchFilter::default()));
        let cb_filter = Arc::clone(&filter);
        let group = crate::watch::DirGroup::new_changes(move |paths| {
            let hit = cb_filter.lock().unwrap().matches(paths);
            if hit {
                on_change();
            }
        })?;
        let mut w = Self { roots, filter, group };
        w.refresh();
        Some(w)
    }

    fn refresh(&mut self) {
        let mut watch_dirs = BTreeSet::new();
        let mut active_dirs = BTreeSet::new();
        let mut pending_targets = BTreeSet::new();

        for root in &self.roots {
            if root.is_dir() {
                watch_dirs.insert(root.clone());
                active_dirs.insert(root.clone());
                if let Ok(entries) = std::fs::read_dir(root) {
                    for entry in entries.flatten() {
                        // `file_type()` does not follow symlinks, matching Qt's QDir::NoSymLinks.
                        if entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                            let sub = entry.path();
                            watch_dirs.insert(sub.clone());
                            active_dirs.insert(sub);
                        }
                    }
                }
            } else {
                // Watch only the nearest existing ancestor and filter strictly for the missing child.
                let mut target = root.as_path();
                while let Some(parent) = target.parent() {
                    if parent.as_os_str().is_empty() {
                        break;
                    }
                    if parent.is_dir() {
                        watch_dirs.insert(parent.to_path_buf());
                        pending_targets.insert(target.to_path_buf());
                        break;
                    }
                    target = parent;
                }
            }
        }

        {
            let mut f = self.filter.lock().unwrap();
            f.active_dirs = active_dirs;
            f.pending_targets = pending_targets;
        }
        self.group.set_dirs(watch_dirs);
    }
}

static DESKTOP_WATCH: Mutex<Option<DesktopWatch>> = Mutex::new(None);

fn desktop_watch_start(notify_cb: usize) {
    let mut guard = DESKTOP_WATCH.lock().unwrap();
    if guard.is_some() || notify_cb == 0 {
        return;
    }
    // SAFETY: caller passes `qsrs_desktop_entries_notify`, an `extern "C" fn()` valid for the process lifetime.
    let notify: extern "C" fn() = unsafe { std::mem::transmute(notify_cb) };
    let roots = desktop_paths().into_iter().map(PathBuf::from).collect();
    *guard = DesktopWatch::new(roots, move || notify());
}

fn desktop_watch_refresh() {
    if let Some(w) = DESKTOP_WATCH.lock().unwrap().as_mut() {
        w.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    #[test]
    fn exec_strings() {
        assert_eq!(parse_exec_string("firefox %u"), ["firefox"]);
        assert_eq!(parse_exec_string("sh -c \"echo a\\\\\\\\b\" 100%%"), ["sh", "-c", "echo a\\b", "100%"]);
        assert_eq!(parse_exec_string("a\\ b 'c d'"), ["a b", "c d"]);
    }

    #[test]
    fn locales_and_actions() {
        let l = Locale::parse("es_AR.UTF-8@euro");
        assert_eq!((l.language.as_str(), l.territory.as_str(), l.modifier.as_str()), ("es", "AR", "euro"));
        let text = "[Desktop Entry]\nType=Application\nName=Files\nName[zz]=Nope\nExec=nautilus --new-window %U\nActions=new;gone;\nCategories=GNOME;Utility;\nNoDisplay=false\n\n[Desktop Action new]\nName=New Window\nExec=nautilus --new-window\n";
        let d = parse_text("org.gnome.Nautilus", text);
        assert_eq!(d.name, "Files");
        assert_eq!(d.command, ["nautilus", "--new-window"]);
        assert_eq!(d.categories, ["GNOME", "Utility"]);
        assert_eq!(d.actions.len(), 1);
        assert_eq!(d.actions[0].name, "New Window");
        let crlf = parse_text("x", "[Desktop Entry] \r\nType=Application\r\nName=Win\r\n  Exec=true\r\n");
        assert_eq!((crlf.name.as_str(), crlf.command.len()), ("Win", 1));
        let link = parse_text("x", "[Desktop Entry]\nType=Link\nName=X\n");
        assert!(link.name.is_empty());
    }

    #[test]
    fn desktop_watch_tracks_changes_and_missing_dirs() {
        let base = std::env::temp_dir().join(format!("qsrs-desktop-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let apps = base.join("applications");

        let hits = Arc::new(AtomicUsize::new(0));
        let cb_hits = Arc::clone(&hits);
        let mut watch = DesktopWatch::new(vec![apps.clone()], move || {
            cb_hits.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();

        let wait_for = |min: usize| {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(2) {
                if hits.load(Ordering::SeqCst) >= min {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            false
        };

        // Unrelated sibling in parent dir must NOT trigger a rescan.
        std::fs::write(base.join("unrelated.txt"), "noise").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        // Creating the missing `applications` directory triggers a rescan.
        std::fs::create_dir(&apps).unwrap();
        assert!(wait_for(1));
        watch.refresh();

        // Adding and removing a `.desktop` file inside `applications` triggers rescans.
        let before = hits.load(Ordering::SeqCst);
        let entry = apps.join("demo.desktop");
        std::fs::write(&entry, "[Desktop Entry]\nType=Application\nName=Demo\n").unwrap();
        assert!(wait_for(before + 1));

        let before = hits.load(Ordering::SeqCst);
        std::fs::remove_file(&entry).unwrap();
        assert!(wait_for(before + 1));

        drop(watch);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
