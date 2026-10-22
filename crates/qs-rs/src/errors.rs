//! `qs errors`: everything a running shell complained about, as a short list. Nothing in the
//! shell is opened or touched: the instance's own log is decoded (every generation since it
//! started), messages that differ only in numbers, ids or addresses fold into one kind, and each
//! kind is put on the side that owns it: the shell (by area, so inir's families read apart),
//! the runtime, or the system around them. `--static` adds a child `check`, which compiles every
//! QML file of the config, all families, without creating anything.

pub use quickshell::errorlog::static_line;
use quickshell::errorlog::{Kind, Owner, Report};
use quickshell::log::Level;
#[cfg(test)]
use quickshell::log::Message;
use std::path::Path;

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
    use quickshell::errorlog::{normalize, owner, RELOADING};

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
