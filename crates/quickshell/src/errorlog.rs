//! What the shell and the runtime complained about, folded into kinds (see `qs errors`), plus
//! `ErrorLog`, the same report live for the shell's own UI: every warning since launch, without
//! opening anything, and a background `check` that compiles every file nobody has opened yet.

use crate::log::{Level, Message};
use core::pin::Pin;
use cxx_qt::{CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::QString;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Owner {
    /// The shell's own code; the area is the folder under the shell dir (`modules/iris` reads
    /// as `iris`), or `?` when the message names no file.
    Shell(String),
    Runtime,
    System,
}

#[derive(Debug)]
pub struct Kind {
    pub owner: Owner,
    pub level: Level,
    pub count: usize,
    /// Times seen in the current generation (since the last reload).
    pub current: usize,
    pub first: i64,
    pub last: i64,
    /// The latest message of this kind, as logged.
    pub sample: String,
    pub category: String,
}

#[derive(Default)]
pub struct Report {
    pub kinds: Vec<Kind>,
    index: HashMap<String, usize>,
    pub reloads: usize,
    pub started: Option<i64>,
}

pub const RELOADING: &str = "Reloading configuration...";

impl Report {
    /// Takes one message; returns the index of its kind when it is the first of that kind.
    pub fn add(&mut self, m: &Message, shell_dir: &str) -> Option<usize> {
        self.started.get_or_insert(m.time);
        if m.body == RELOADING {
            self.reloads += 1;
            for k in &mut self.kinds {
                k.current = 0;
            }
            return None;
        }
        if m.level < Level::Warn {
            return None;
        }
        let body = strip_ansi(&m.body);
        let key = format!("{}|{}", m.category, normalize(&body));
        if let Some(&i) = self.index.get(&key) {
            let k = &mut self.kinds[i];
            k.count += 1;
            k.current += 1;
            k.last = m.time;
            k.sample = body;
            return None;
        }
        self.kinds.push(Kind {
            owner: owner(&m.category, &body, shell_dir),
            level: m.level,
            count: 1,
            current: 1,
            first: m.time,
            last: m.time,
            sample: body,
            category: m.category.clone(),
        });
        self.index.insert(key, self.kinds.len() - 1);
        Some(self.kinds.len() - 1)
    }
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// What stays the same between two messages of one kind: numbers, hex addresses and D-Bus
/// unique names become `#`, so "instance_1_1388", "0x7f3a…" and ":1.234" never split a kind.
pub fn normalize(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            // A hex literal or a run of digits (dots inside keep "1.234" whole).
            if c == '0' && chars.peek() == Some(&'x') {
                chars.next();
                while chars.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    chars.next();
                }
            } else {
                while chars.peek().is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                    chars.next();
                }
            }
            out.push('#');
        } else {
            out.push(c);
        }
    }
    out
}

/// The shell file a message points at, relative to the shell dir: from `file://` URLs or plain
/// absolute paths in the body.
pub fn shell_file<'a>(body: &'a str, shell_dir: &str) -> Option<&'a str> {
    let dir = shell_dir.trim_end_matches('/');
    let at = body.find(dir)?;
    let rest = body[at + dir.len()..].trim_start_matches('/');
    let end = rest.find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ')').unwrap_or(rest.len());
    Some(&rest[..end])
}

pub fn owner(category: &str, body: &str, shell_dir: &str) -> Owner {
    if let Some(file) = shell_file(body, shell_dir) {
        let mut parts = file.split('/');
        let area = match (parts.next(), parts.next()) {
            (Some("modules"), Some(a)) if !a.contains('.') => a.to_string(),
            (Some(top), Some(_)) => top.to_string(),
            _ => "root".to_string(),
        };
        return Owner::Shell(area);
    }
    // Things the shell asked for that the machine doesn't have (a daemon that isn't installed or
    // won't start): nothing either side can fix in code.
    let system = ["Could not activate remote peer", "unit failed", "was not provided by any .service files"];
    if system.iter().any(|s| body.contains(s)) {
        return Owner::System;
    }
    if body.contains("qrc:/qt/qml/Quickshell") || category.starts_with("quickshell") || category.starts_with("qt.") {
        return Owner::Runtime;
    }
    // A console.warn or an error from shell JavaScript without a location.
    Owner::Shell("?".into())
}

/// One `check` line, `path:line:col: message`, as (area, text). Paths are relative to the shell.
pub fn static_line(line: &str) -> Option<(String, String)> {
    let (path, _) = line.split_once(".qml:")?;
    if path.contains(' ') {
        return None;
    }
    let area = owner("", &format!("/s/{line}"), "/s");
    let area = match area {
        Owner::Shell(a) => a,
        _ => "?".into(),
    };
    Some((area, line.to_string()))
}


/// The running instance's report, fed by the logger from the first message on.
struct Live {
    report: Report,
    root: String,
    shell_dir: String,
    checked: Vec<(String, String)>,
    summary: String,
}

fn live() -> &'static Mutex<Live> {
    static L: OnceLock<Mutex<Live>> = OnceLock::new();
    L.get_or_init(|| {
        Mutex::new(Live { report: Report::default(), root: String::new(), shell_dir: "\0".into(), checked: Vec::new(), summary: String::new() })
    })
}

/// The shell's root file; messages naming files under its folder belong to the shell.
pub fn set_root(root: &Path) {
    let mut l = live().lock().unwrap();
    l.root = root.to_string_lossy().into_owned();
    l.shell_dir = root.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
}

/// Called by the logger for every message; never logs itself.
pub(crate) fn feed(m: &Message) {
    if m.level < Level::Warn && m.body != RELOADING {
        return;
    }
    {
        let mut l = live().lock().unwrap();
        let dir = l.shell_dir.clone();
        l.report.add(m, &dir);
    }
    ping();
}

static WATCHERS: Mutex<Vec<CxxQtThread<qobject::ErrorLog>>> = Mutex::new(Vec::new());
static PENDING: AtomicBool = AtomicBool::new(false);

/// Wakes every ErrorLog at most four times a second, so a flood of warnings costs one rebuild.
fn ping() {
    if PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = std::thread::Builder::new().name("qs-errorlog".into()).spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(250));
        PENDING.store(false, Ordering::Release);
        WATCHERS.lock().unwrap().retain(|w| w.queue(|this| this.refresh()).is_ok());
    });
}

fn esc(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `{reloads, started, kinds: [{owner, area, level, count, current, first, last, text, category}],
/// checked: [{area, text}], summary}`; `summary` is empty until a check finished.
fn snapshot() -> (String, i32) {
    let l = live().lock().unwrap();
    let r = &l.report;
    let mut s = format!("{{\"reloads\":{},\"started\":{},\"kinds\":[", r.reloads, r.started.unwrap_or(0));
    let mut live_kinds = 0;
    for (i, k) in r.kinds.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let (owner, area) = match &k.owner {
            Owner::Shell(a) => ("shell", a.as_str()),
            Owner::Runtime => ("runtime", ""),
            Owner::System => ("system", ""),
        };
        if k.current > 0 {
            live_kinds += 1;
        }
        s.push_str(&format!("{{\"owner\":\"{owner}\",\"area\":"));
        esc(area, &mut s);
        s.push_str(&format!(
            ",\"level\":\"{}\",\"count\":{},\"current\":{},\"first\":{},\"last\":{},\"text\":",
            if k.level >= Level::Critical { "error" } else { "warn" },
            k.count,
            k.current,
            k.first,
            k.last
        ));
        esc(&k.sample, &mut s);
        s.push_str(",\"category\":");
        esc(&k.category, &mut s);
        s.push('}');
    }
    s.push_str("],\"checked\":[");
    for (i, (area, text)) in l.checked.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"area\":");
        esc(area, &mut s);
        s.push_str(",\"text\":");
        esc(text, &mut s);
        s.push('}');
    }
    s.push_str("],\"summary\":");
    esc(&l.summary, &mut s);
    s.push('}');
    (s, live_kinds + l.checked.len() as i32)
}

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, json, READ, NOTIFY)]
        #[qproperty(i32, count, READ, NOTIFY)]
        #[qproperty(bool, checking, READ, NOTIFY)]
        type ErrorLog = super::ErrorLogRust;

        /// Compiles every QML file of the shell in a child `check`, at the lowest priority.
        #[qinvokable]
        fn check(self: Pin<&mut ErrorLog>);
    }

    impl cxx_qt::Threading for ErrorLog {}
    impl cxx_qt::Initialize for ErrorLog {}
}

#[derive(Default)]
pub struct ErrorLogRust {
    json: QString,
    count: i32,
    checking: bool,
}

impl cxx_qt::Initialize for qobject::ErrorLog {
    fn initialize(self: Pin<&mut Self>) {
        WATCHERS.lock().unwrap().push(self.qt_thread());
        self.refresh();
    }
}

impl qobject::ErrorLog {
    fn refresh(mut self: Pin<&mut Self>) {
        let (json, count) = snapshot();
        let json = QString::from(&json);
        if json != self.json {
            self.as_mut().rust_mut().json = json;
            self.as_mut().json_changed();
        }
        if count != self.count {
            self.as_mut().rust_mut().count = count;
            self.as_mut().count_changed();
        }
    }

    fn check(mut self: Pin<&mut Self>) {
        let root = live().lock().unwrap().root.clone();
        if self.checking || root.is_empty() {
            return;
        }
        self.as_mut().rust_mut().checking = true;
        self.as_mut().checking_changed();
        let qt = self.qt_thread();
        let _ = std::thread::Builder::new().name("qs-check".into()).spawn(move || {
            let exe = std::env::current_exe().unwrap_or_else(|_| "qs".into());
            let out = std::process::Command::new("nice")
                .arg("-n19")
                .arg(exe)
                .args(["check", "-p", &root])
                .env("QT_QPA_PLATFORM", "offscreen")
                .env("NO_COLOR", "1")
                .stdin(std::process::Stdio::null())
                .output();
            let text = out.map(|o| format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))).unwrap_or_default();
            {
                let mut l = live().lock().unwrap();
                l.summary = text.lines().find(|l| l.contains("files checked")).unwrap_or("check did not finish").to_string();
                let mut lines: Vec<_> = text.lines().filter_map(static_line).collect();
                lines.sort();
                lines.dedup();
                l.checked = lines;
            }
            let _ = qt.queue(|mut this| {
                this.as_mut().rust_mut().checking = false;
                this.as_mut().checking_changed();
            });
            ping();
        });
    }
}
