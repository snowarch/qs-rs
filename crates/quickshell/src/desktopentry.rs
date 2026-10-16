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
    }
}

use ffi::{DesktopActionData, DesktopEntryData};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Default, Clone, PartialEq, Debug)]
struct Locale {
    language: String,
    territory: String,
    modifier: String,
}

impl Locale {
    /// `lang_COUNTRY.ENCODING@MODIFIER`
    fn parse(s: &str) -> Self {
        let mut end = s.len();
        let mut locale = Locale::default();
        if let Some(i) = s.find('@') {
            locale.modifier = s[i + 1..].to_string();
            end = i;
        }
        if let Some(i) = s.find('.') {
            end = i;
        }
        if let Some(i) = s.find('_') {
            locale.territory = s[i + 1..end.max(i + 1)].to_string();
            end = i;
        }
        locale.language = s[..end].to_string();
        locale
    }

    fn is_valid(&self) -> bool {
        !self.language.is_empty()
    }

    fn match_score(&self, other: &Locale) -> i32 {
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

fn split_list(value: &str) -> Vec<String> {
    value.split(';').filter(|s| !s.is_empty()).map(String::from).collect()
}

pub fn parse_text(id: &str, text: &str) -> DesktopEntryData {
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
    let mut group = String::new();
    // Insertion order kept so later keys win the same way upstream's QHash replacement does.
    let mut entries: Vec<(String, Locale, String)> = Vec::new();
    let mut action_order: Vec<String> = Vec::new();
    let mut pending: HashMap<String, DesktopActionData> = HashMap::new();

    let mut finish = |group: &str, entries: &mut Vec<(String, Locale, String)>, data: &mut DesktopEntryData| {
        if group == "Desktop Entry" {
            let ty = entries.iter().find(|(k, ..)| k == "Type").map(|(.., v)| v.as_str());
            // Upstream returns before clearing here, so these keys leak into the next group.
            if ty != Some("Application") {
                return;
            }
            for (key, _, value) in entries.iter() {
                let value = value.clone();
                match key.as_str() {
                    "Name" => data.name = value,
                    "GenericName" => data.generic_name = value,
                    "StartupWMClass" => data.startup_class = value,
                    "NoDisplay" => data.no_display = value == "true",
                    "Hidden" => data.hidden = value == "true",
                    "Comment" => data.comment = value,
                    "Icon" => data.icon = value,
                    "Exec" => {
                        data.command = parse_exec_string(&value);
                        data.exec_string = value;
                    }
                    "Path" => data.working_directory = value,
                    "Terminal" => data.terminal = value == "true",
                    "Categories" => data.categories = split_list(&value),
                    "Keywords" => data.keywords = split_list(&value),
                    "Actions" => action_order = split_list(&value),
                    _ => {}
                }
            }
        } else if let Some(name) = group.strip_prefix("Desktop Action ") {
            let mut action = DesktopActionData { id: name.to_string(), name: String::new(), icon: String::new(), exec_string: String::new(), command: Vec::new() };
            for (key, _, value) in entries.iter() {
                match key.as_str() {
                    "Name" => action.name = value.clone(),
                    "Icon" => action.icon = value.clone(),
                    "Exec" => {
                        action.command = parse_exec_string(value);
                        action.exec_string = value.clone();
                    }
                    _ => {}
                }
            }
            pending.insert(name.to_string(), action);
        }
        entries.clear();
    };

    for line in text.split('\n').filter(|l| !l.is_empty()) {
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
            finish(&group, &mut entries, &mut data);
            group = line[1..line.len() - 1].to_string();
            continue;
        }
        let Some(split) = line.find('=') else {
            continue;
        };
        let (mut key, value) = (&line[..split], &line[split + 1..]);
        let mut locale = Locale::default();
        if let Some(i) = key.find('[') {
            if i != key.len() - 1 {
                let end = if key.ends_with(']') { key.len() - 1 } else { key.len() };
                locale = Locale::parse(&key[i + 1..end.max(i + 1)]);
                key = &key[..i];
            }
        }
        match entries.iter_mut().find(|(k, ..)| k == key) {
            Some(old) => {
                let old_score = system.match_score(&old.1);
                let new_score = system.match_score(&locale);
                if new_score > old_score || (old_score == 0 && !locale.is_valid()) {
                    old.1 = locale;
                    old.2 = value.to_string();
                }
            }
            None => entries.push((key.to_string(), locale, value.to_string())),
        }
    }
    finish(&group, &mut entries, &mut data);
    drop(finish);
    for id in &action_order {
        if let Some(a) = pending.get(id) {
            data.actions.push(DesktopActionData { id: a.id.clone(), name: a.name.clone(), icon: a.icon.clone(), exec_string: a.exec_string.clone(), command: a.command.clone() });
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
    if home.is_empty() {
        if let Ok(h) = std::env::var("HOME") {
            home = format!("{h}/.local/share");
        }
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
    let mut names: Vec<_> = read.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    // QDir's default order: by name, case-insensitive.
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
    for name in names {
        let path = dir.join(&name);
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.is_dir() {
            let sub = if prefix.is_empty() { name.clone() } else { format!("{prefix}-{name}") };
            scan_dir(&path, &sub, out);
        } else if meta.is_file() && name.ends_with(".desktop") {
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
        eprintln!("Cannot start process as command is empty.");
        return;
    }
    crate::proc::spawn_detached(&crate::proc::Context {
        command: command.clone(),
        working_directory: working_directory.to_string(),
        environment: Vec::new(),
        clear_environment: false,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let link = parse_text("x", "[Desktop Entry]\nType=Link\nName=X\n");
        assert!(link.name.is_empty());
    }
}
