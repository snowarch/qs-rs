//! `qs errors`: everything a running shell complained about, as a short list. Nothing in the
//! shell is opened or touched: the instance's own log is decoded (every generation since it
//! started), messages that differ only in numbers, ids or addresses fold into one kind, and each
//! kind is put on the side that owns it: the shell (by area, so inir's families read apart),
//! the runtime, or the system around them. `--static` adds a child `check`, which compiles every
//! QML file of the config, all families, without creating anything.

use quickshell::log::{Level, Message};
use std::collections::HashMap;
use std::path::Path;

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

const RELOADING: &str = "Reloading configuration...";

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

fn ago(now: i64, then: i64) -> String {
    let s = ((now - then) / 1000).max(0);
    match s {
        0..=59 => format!("{s}s ago"),
        60..=3599 => format!("{}m ago", s / 60),
        _ => format!("{}h{:02}m ago", s / 3600, s / 60 % 60),
    }
}

fn owner_title(o: &Owner) -> String {
    match o {
        Owner::Shell(a) => format!("shell · {a}"),
        Owner::Runtime => "runtime".into(),
        Owner::System => "system".into(),
    }
}

pub fn print(report: &Report, now: i64, color: bool) {
    let (bold, dim, red, yellow, reset) = if color { ("\x1b[1m", "\x1b[2m", "\x1b[31m", "\x1b[33m", "\x1b[0m") } else { ("", "", "", "", "") };
    let mut order: Vec<&Kind> = report.kinds.iter().collect();
    order.sort_by(|a, b| a.owner.cmp(&b.owner).then(b.current.cmp(&a.current)).then(b.count.cmp(&a.count)));
    let mut last_owner = None;
    for k in order {
        if last_owner != Some(&k.owner) {
            let n = report.kinds.iter().filter(|o| o.owner == k.owner).count();
            println!("\n{bold}{}{reset} {dim}({n} kind{}){reset}", owner_title(&k.owner), if n == 1 { "" } else { "s" });
            last_owner = Some(&k.owner);
        }
        let mark = if k.level >= Level::Critical { format!("{red}E{reset}") } else { format!("{yellow}W{reset}") };
        let now_note = if k.current > 0 { format!("{} since reload", k.current) } else { "not since reload".into() };
        println!("  {mark} {:>4}x  {}", k.count, k.sample);
        println!("  {dim}        {} · {now_note} · first {} · last {}{reset}", k.category, ago(now, k.first), ago(now, k.last));
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// `qs errors` on a log file: the grouped report, then with `follow` every new kind as it shows
/// up. `static_lines` comes from a `check` of the same shell.
pub fn run(log: &Path, shell_dir: &str, follow: bool, static_lines: Option<(Vec<(String, String)>, String)>, color: bool) -> i32 {
    let mut report = Report::default();
    if let Err(e) = quickshell::log::each_message(log, false, |m| {
        report.add(m, shell_dir);
    }) {
        eprintln!("{e}");
        return 255;
    }
    let now = now_ms();
    let up = report.started.map_or(String::new(), |s| ago(now, s).replace(" ago", ""));
    println!("{} kinds of warnings and errors · up {up} · {} reload{}", report.kinds.len(), report.reloads, if report.reloads == 1 { "" } else { "s" });
    print(&report, now, color);
    let mut found = !report.kinds.is_empty();
    if let Some((lines, summary)) = static_lines {
        println!("\n{}static check{} · {summary}", if color { "\x1b[1m" } else { "" }, if color { "\x1b[0m" } else { "" });
        let mut lines = lines;
        lines.sort();
        for (area, text) in &lines {
            println!("  {area:<8} {text}");
        }
        found |= !lines.is_empty();
    }
    if follow {
        println!("\nfollowing; new kinds show up here");
        let seen = report.kinds.len();
        let mut fresh = Report::default();
        // Replays the file into a fresh report so kinds keep their identity, then prints only
        // kinds born after the summary.
        let mut n = 0usize;
        let _ = quickshell::log::each_message(log, true, |m| {
            if let Some(i) = fresh.add(m, shell_dir) {
                n += 1;
                if n > seen {
                    let k = &fresh.kinds[i];
                    println!("  new  {}  {}", owner_title(&k.owner), k.sample);
                }
            }
        });
    }
    if found { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(level: Level, cat: &str, body: &str) -> Message {
        Message { level, category: cat.into(), body: body.into(), time: 0 }
    }

    #[test]
    fn numbers_ids_and_addresses_fold() {
        assert_eq!(
            normalize("reading Position of org.mpris.MediaPlayer2.firefox.instance_1_1388 failed"),
            normalize("reading Position of org.mpris.MediaPlayer2.firefox.instance_1_77 failed")
        );
        assert_eq!(normalize("object 0x7f3a2b at :1.234"), "object # at :#");
        assert_ne!(normalize("Position failed"), normalize("Metadata failed"));
    }

    #[test]
    fn owners() {
        let dir = "/home/u/inir";
        assert_eq!(owner("qml", "file:///home/u/inir/modules/iris/bar/IrisBar.qml:12: TypeError", dir), Owner::Shell("iris".into()));
        assert_eq!(owner("qml", "file:///home/u/inir/services/Audio.qml:3: x", dir), Owner::Shell("services".into()));
        assert_eq!(owner("default", "qrc:/qt/qml/Quickshell/qml/QsMenuOpener.qml:21: TypeError", dir), Owner::Runtime);
        assert_eq!(owner("quickshell.dbus", "Mpris: reading Position failed", dir), Owner::Runtime);
        assert_eq!(owner("quickshell.bluetooth", "NameHasNoOwner: Could not activate remote peer 'org.bluez': unit failed", dir), Owner::System);
        assert_eq!(owner("qml", "something went wrong", dir), Owner::Shell("?".into()));
    }

    #[test]
    fn kinds_count_and_reset_on_reload() {
        let dir = "/s";
        let mut r = Report::default();
        assert!(r.add(&msg(Level::Warn, "quickshell.dbus", "Event(3, closed) failed"), dir).is_some());
        assert!(r.add(&msg(Level::Warn, "quickshell.dbus", "Event(9, closed) failed"), dir).is_none());
        assert!(r.add(&msg(Level::Info, "default", "Configuration Loaded"), dir).is_none());
        assert_eq!((r.kinds.len(), r.kinds[0].count, r.kinds[0].current), (1, 2, 2));
        r.add(&msg(Level::Info, "default", RELOADING), dir);
        assert_eq!((r.reloads, r.kinds[0].current), (1, 0));
        r.add(&msg(Level::Warn, "quickshell.dbus", "\x1b[33mEvent(4, closed) failed\x1b[0m"), dir);
        assert_eq!((r.kinds[0].count, r.kinds[0].current), (3, 1));
    }

    #[test]
    fn static_lines_by_area() {
        assert_eq!(
            static_line("modules/waffle/sessionScreen/PowerButton.qml:13:1: Type X unavailable"),
            Some(("waffle".into(), "modules/waffle/sessionScreen/PowerButton.qml:13:1: Type X unavailable".into()))
        );
        assert_eq!(static_line("1514 files checked, 3 errors"), None);
    }
}
