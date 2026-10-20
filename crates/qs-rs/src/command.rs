//! What the `qs` subcommands do (upstream launch/command.cpp): config lookup, instance selection,
//! `list`, `kill`, `ipc`, `log` and `--version`. Messages upstream prints through its bare log
//! category go to stdout like there (inir reads `list` output); IPC errors go to stderr and exit
//! 255, where upstream prints them on stdout and exits 0 (inir's `ipc call` retry needs the code).

use crate::cli::{Cli, Ipc};
use quickshell::instance::{self, InstanceInfo};
use std::path::{Path, PathBuf};

/// Upstream release whose behaviour qs-rs follows; inir reads it as the second word of
/// `--version`.
pub const UPSTREAM_VERSION: &str = "0.3.2";

pub fn version() -> String {
    format!(
        "{UPSTREAM_VERSION} (revision {}, distributed by qs-rs {})",
        env!("QSRS_REVISION"),
        env!("CARGO_PKG_VERSION")
    )
}

fn config_base_dirs() -> Vec<PathBuf> {
    let env = |k| std::env::var(k).ok().filter(|v: &String| !v.is_empty());
    let home = env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    let mut dirs = vec![home];
    match env("XDG_CONFIG_DIRS") {
        Some(d) => dirs.extend(d.split(':').filter(|s| !s.is_empty()).map(PathBuf::from)),
        None => dirs.push("/etc/xdg".into()),
    }
    dirs.into_iter().map(|d| d.join("quickshell")).collect()
}

/// A `shell.qml` directly in a base dir is the "default" config and hides its subfolders.
fn locate_named_config(name: &str) -> Option<PathBuf> {
    for base in config_base_dirs() {
        let shell = base.join("shell.qml");
        if shell.is_file() {
            if name == "default" {
                return Some(shell);
            }
            continue;
        }
        let shell = base.join(name).join("shell.qml");
        if shell.is_file() {
            return Some(shell);
        }
    }
    None
}

fn manifest_entry(cli: &Cli) -> Result<Option<PathBuf>, ()> {
    let manifest = match &cli.manifest {
        Some(m) => PathBuf::from(m),
        None => {
            let default = config_base_dirs().remove(0).join("manifest.conf");
            if !default.is_file() {
                return Ok(None);
            }
            default
        }
    };
    eprintln!("Config manifests (manifest.conf) are deprecated and will be removed in a future release.");
    eprintln!("Consider using symlinks to a subfolder of quickshell's XDG config dirs.");
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        eprintln!("Could not open manifest at path {:?}", manifest.display().to_string());
        return Err(());
    };
    let name = cli.name.as_deref().unwrap_or_default();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let parts: Vec<&str> = line.split('=').collect();
        if parts.len() != 2 {
            eprintln!("Manifest line not in expected format 'name = relativepath': {line:?}");
            return Err(());
        }
        if parts[0].trim() == name {
            let base = instance::absolute_path(&manifest);
            return Ok(Some(base.parent().unwrap_or(Path::new("/")).join(parts[1].trim())));
        }
    }
    println!("Configuration {name:?} not found when searching manifest {:?}", manifest.display().to_string());
    Err(())
}

/// The config's shell file as an absolute path, symlinks kept (what upstream hashes into the
/// path id). Prints why on failure.
pub fn locate_config_file(cli: &Cli) -> Result<PathBuf, ()> {
    let path = match &cli.path {
        Some(p) => PathBuf::from(p),
        None => match manifest_entry(cli)? {
            Some(p) => p,
            None => {
                let name = cli.name.as_deref().unwrap_or("default");
                return match locate_named_config(name) {
                    Some(p) => Ok(instance::absolute_path(&p)),
                    None if name == "default" => {
                        println!("Could not find \"default\" config directory or shell.qml in any valid config path.");
                        Err(())
                    }
                    None => {
                        println!("Could not find {name:?} config directory in any valid config path.");
                        Err(())
                    }
                };
            }
        },
    };
    let path = if path.is_dir() { path.join("shell.qml") } else { path };
    if !path.is_file() {
        println!("Could not open config file at {:?}", path.display().to_string());
        return Err(());
    }
    Ok(instance::absolute_path(&path))
}

fn by_path(config: &Path) -> PathBuf {
    instance::base_run_dir().join("by-path").join(instance::path_id(&config.to_string_lossy()))
}

fn display_filter(cli: &Cli) -> String {
    if cli.any_display { String::new() } else { instance::display_connection() }
}

fn sort(list: &mut [InstanceInfo], newest_first: bool) {
    list.sort_by_key(|i| i.launch_time_ms);
    if newest_first {
        list.reverse();
    }
}

/// The instance a command acts on (upstream selectInstance): `--pid`, a unique `--id` prefix,
/// or the config's oldest live instance (newest with `-n`; dead ones only with `dead_fallback`).
/// Returns the instance and whether it is dead.
pub fn select_instance(cli: &Cli, dead_fallback: bool, log_help: bool) -> Result<(InstanceInfo, bool), ()> {
    let base = instance::base_run_dir();
    if let Some(pid) = cli.pid {
        return match instance::check_lock(&base.join("by-pid").join(pid.to_string())) {
            Some((lock_pid, info)) if lock_pid != -1 || dead_fallback => Ok((info, lock_pid == -1)),
            _ => {
                if log_help {
                    println!("No instance found for pid {pid}");
                }
                Err(())
            }
        };
    }
    if let Some(id) = &cli.id {
        let (mut live, mut other, mut dead) = instance::collect(&base.join("by-pid"), &display_filter(cli));
        for list in [&mut live, &mut other, &mut dead] {
            list.retain(|i| i.instance_id.starts_with(id.as_str()));
        }
        let use_dead = live.is_empty() && dead_fallback;
        let found = if use_dead { &dead } else { &live };
        if found.len() == 1 {
            return Ok((found[0].clone(), use_dead));
        }
        if log_help {
            if found.is_empty() {
                if !other.is_empty() {
                    println!("No running instances on the current display {:?} start with {id:?}", instance::display_connection());
                    println!("Some instances on other displays match:");
                    for i in &other {
                        println!(" - {} ({})", i.instance_id, i.display);
                    }
                }
                if dead_fallback {
                    println!("No instances start with {id:?}");
                } else {
                    println!("No running instances start with {id:?}");
                    if !dead.is_empty() {
                        println!("Some dead instances match:");
                        for i in &dead {
                            println!(" - {}", i.instance_id);
                        }
                    }
                }
            } else {
                println!("More than one instance starts with {id:?}");
                for i in found {
                    println!(" - {}{}", i.instance_id, if use_dead { " (dead)" } else { "" });
                }
            }
        }
        return Err(());
    }
    let config = locate_config_file(cli)?;
    let (live, other, mut dead) = instance::collect(&by_path(&config), &display_filter(cli));
    let use_dead = live.is_empty() && dead_fallback && !dead.is_empty();
    let mut found = if use_dead { dead.clone() } else { live.clone() };
    // Upstream sorts newest first when the candidates are dead (the last log is the useful one).
    sort(&mut found, cli.newest || use_dead);
    if let Some(i) = found.into_iter().next() {
        return Ok((i, use_dead));
    }
    if log_help {
        let shown = config.display().to_string();
        if live.is_empty() && dead.len() > 1 {
            println!("No running instances for {shown:?}");
            println!("Dead instances:");
            sort(&mut dead, cli.newest);
            for i in &dead {
                println!(" - {}", i.instance_id);
            }
        } else if !other.is_empty() {
            println!("No running instances for {shown:?}  present on the current display {:?}", instance::display_connection());
            println!("Some instances on other displays match:");
            for i in &other {
                println!(" - {} ({})", i.instance_id, i.display);
            }
        } else {
            println!("No running instances for {shown:?}");
        }
    }
    Err(())
}

fn local_time(ms: i64) -> libc::tm {
    let secs = (ms.div_euclid(1000)) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    tm
}

fn format_time(ms: i64, sep: char) -> String {
    let t = local_time(ms);
    format!("{:04}-{:02}-{:02}{sep}{:02}:{:02}:{:02}", t.tm_year + 1900, t.tm_mon + 1, t.tm_mday, t.tm_hour, t.tm_min, t.tm_sec)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// QJsonDocument::Indented: four spaces, keys in order.
fn list_json(instances: &[(InstanceInfo, bool)]) -> String {
    let mut out = String::from("[\n");
    for (n, (i, _)) in instances.iter().enumerate() {
        out += &format!(
            "    {{\n        \"config_path\": {},\n        \"id\": {},\n        \"launch_time\": {},\n        \"pid\": {},\n        \"shell_id\": {}\n    }}{}\n",
            json_string(&i.config_path),
            json_string(&i.instance_id),
            json_string(&format_time(i.launch_time_ms, 'T')),
            i.pid,
            json_string(&i.shell_id),
            if n + 1 < instances.len() { "," } else { "" }
        );
    }
    out + "]\n"
}

fn list_text(instances: &[(InstanceInfo, bool)], color: bool, now: i64) -> String {
    let mut out = String::new();
    for (i, dead) in instances {
        let gray = color && *dead;
        let run = (now - i.launch_time_ms) / 1000;
        let running = format!(" (running for {} hours, {} minutes, {} seconds)", run / 3600, run / 60 % 60, run % 60);
        out += &format!(
            "{}Instance {}{}:\n  Process ID: {}\n  Shell ID: {}\n  Config path: {}\n  Display connection: {}\n  Launch time: {}{}\n{}\n",
            if gray { "\x1b[90m" } else { "" },
            i.instance_id,
            if *dead { " (dead)" } else { "" },
            i.pid,
            i.shell_id,
            i.config_path,
            i.display,
            format_time(i.launch_time_ms, ' '),
            if *dead { "" } else { &running },
            if gray { "\x1b[0m" } else { "" },
        );
    }
    out
}

pub fn list(cli: &Cli) -> i32 {
    let (dir, config) = if cli.all {
        (instance::base_run_dir().join("by-pid"), None)
    } else {
        match locate_config_file(cli) {
            Ok(c) => (by_path(&c), Some(c)),
            Err(()) => {
                println!("Use --all to list all instances.");
                return 255;
            }
        }
    };
    let filter = if cli.all { String::new() } else { display_filter(cli) };
    let (mut live, _, mut dead) = instance::collect(&dir, &filter);
    sort(&mut live, cli.newest);
    let mut instances: Vec<(InstanceInfo, bool)> = Vec::new();
    if cli.show_dead {
        sort(&mut dead, cli.newest);
        instances.extend(dead.into_iter().map(|i| (i, true)));
    }
    instances.extend(live.into_iter().map(|i| (i, false)));
    if instances.is_empty() {
        match config {
            None => println!("No running instances."),
            Some(c) => {
                println!("No running instances for {:?}", c.display().to_string());
                println!("Use --all to list all instances.");
            }
        }
    } else if cli.json {
        print!("{}", list_json(&instances));
    } else {
        print!("{}", list_text(&instances, !cli.no_color, now_ms()));
    }
    0
}

pub fn kill(cli: &Cli) -> i32 {
    let Ok((info, _)) = select_instance(cli, false, true) else { return 255 };
    match quickshell_io::ipc::kill(&info.instance_id, std::time::Duration::from_secs(30)) {
        Ok(()) => {
            println!("Killed {}", info.instance_id);
            0
        }
        Err(e) => {
            eprintln!("Could not reach instance {}: {e}", info.instance_id);
            255
        }
    }
}

pub fn ipc(cli: &Cli, cmd: &Ipc) -> i32 {
    use quickshell_io::ipc::{Command, read_call_reply, read_listen_reply, read_property_reply, read_query_reply, request, stream};
    let required = |what: &[(&String, &str)]| {
        what.iter().find(|(v, _)| v.is_empty()).map(|(_, msg)| {
            eprintln!("{msg}");
            255
        })
    };
    let check = match cmd {
        Ipc::Call { target, function, .. } => {
            required(&[(target, "Target required to send message."), (function, "Function required to send message.")])
        }
        Ipc::PropGet { target, property } => {
            required(&[(target, "Target required to send message."), (property, "Property required to send message.")])
        }
        Ipc::Wait { target, signal } | Ipc::Listen { target, signal } => {
            required(&[(target, "Target required to listen for signals."), (signal, "Signal required to listen.")])
        }
        Ipc::Show { .. } => None,
    };
    if let Some(code) = check {
        return code;
    }
    let Ok((info, _)) = select_instance(cli, false, true) else { return 255 };
    let id = &info.instance_id;
    let result = match cmd {
        Ipc::Call { target, function, args } => {
            let n = args.len();
            let c = Command::Call { target: target.clone(), function: function.clone(), args: args.clone() };
            request(id, &c, |i, r| read_call_reply(i, r, n)).map(|r| r.map(Option::unwrap_or_default))
        }
        Ipc::Show { target, name } => request(id, &Command::Query { target: target.clone(), name: name.clone() }, read_query_reply),
        Ipc::PropGet { target, property } => {
            request(id, &Command::Property { target: target.clone(), property: property.clone() }, read_property_reply)
        }
        Ipc::Wait { target, signal } | Ipc::Listen { target, signal } => {
            let once = matches!(cmd, Ipc::Wait { .. });
            let c = Command::Listen { target: target.clone(), signal: signal.clone() };
            let mut failed = None;
            let r = stream(id, &c, read_listen_reply, |reply| match reply {
                Ok(value) => {
                    println!("{value}");
                    !once
                }
                Err(msg) => {
                    failed = Some(msg);
                    false
                }
            });
            r.map(|()| failed.map_or(Ok(String::new()), Err))
        }
    };
    match result {
        Ok(Ok(text)) => {
            if !text.is_empty() {
                println!("{text}");
            }
            0
        }
        Ok(Err(msg)) => {
            eprintln!("{msg}");
            255
        }
        Err(e) => {
            eprintln!("Could not reach instance {id}: {e}");
            255
        }
    }
}

/// `log`: a given file, or the selected instance's detailed log (live, else the newest dead).
pub fn log(cli: &Cli, file: Option<&str>, tail: usize, follow: bool, rules: &str) -> i32 {
    let path = match file {
        Some(f) => PathBuf::from(f),
        None => match select_instance(cli, true, true) {
            Ok((info, _)) => instance::base_run_dir().join("by-id").join(info.instance_id).join("log.qslog"),
            Err(()) => return 255,
        },
    };
    let color = !cli.no_color && std::env::var("NO_COLOR").map_or(true, |v| v.is_empty());
    quickshell::log::read_file(&path, tail, follow, rules, color, cli.log_times)
}

/// `qs errors`: the running instance's warnings and errors, grouped (see errors.rs).
pub fn errors(cli: &Cli, follow: bool, static_check: bool) -> i32 {
    let info = match select_instance(cli, true, true) {
        Ok((info, _)) => info,
        Err(()) => return 255,
    };
    let log = instance::base_run_dir().join("by-id").join(&info.instance_id).join("log.qslog");
    let config = PathBuf::from(&info.config_path);
    let shell_dir = config.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    let static_lines = static_check.then(|| {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("qs"));
        let out = std::process::Command::new(exe)
            .args(["check", "-p", &info.config_path])
            .env("QT_QPA_PLATFORM", "offscreen")
            .env("NO_COLOR", "1")
            .output();
        let text = out.map(|o| format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))).unwrap_or_default();
        let summary = text.lines().find(|l| l.contains("files checked")).unwrap_or("check did not finish").to_string();
        (text.lines().filter_map(crate::errors::static_line).collect(), summary)
    });
    let color = !cli.no_color && std::env::var("NO_COLOR").map_or(true, |v| v.is_empty());
    crate::errors::run(&log, &shell_dir, follow, static_lines, color)
}

/// `qs frames`: the instance's frame meter (frames.h); a FILE is made absolute here, since the
/// instance runs elsewhere.
pub fn frames(cli: &Cli, action: &str, arg: &str) -> i32 {
    let Ok((info, _)) = select_instance(cli, false, true) else { return 255 };
    let arg = if action == "start" && !arg.is_empty() && arg != "-" {
        std::path::absolute(arg).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| arg.to_string())
    } else {
        arg.to_string()
    };
    let cmd = quickshell_io::ipc::Command::Frames { action: action.to_string(), arg };
    match quickshell_io::ipc::request(&info.instance_id, &cmd, quickshell_io::ipc::read_property_reply) {
        Ok(Ok(out)) => {
            println!("{out}");
            0
        }
        Ok(Err(_)) => {
            eprintln!("This instance has no frame meter (not qs-rs, or older than it).");
            255
        }
        Err(e) => {
            eprintln!("Could not reach the instance: {e}");
            255
        }
    }
}

fn gpu_info() -> String {
    let mut out = String::new();
    let mut nodes: Vec<PathBuf> = std::fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("renderD")))
        .collect();
    nodes.sort();
    for node in nodes {
        let dev = node.join("device");
        let read = |f: &str| std::fs::read_to_string(dev.join(f)).map(|s| s.trim().trim_start_matches("0x").to_string()).unwrap_or_else(|_| "unknown".into());
        let link = |f: &str| std::fs::read_link(dev.join(f)).ok().and_then(|l| l.file_name().map(|n| n.to_string_lossy().into_owned()));
        let address = std::fs::canonicalize(&dev).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        out += &format!(
            "GPU /dev/dri/{}\n  Driver: {}\n  Model: {}:{}\n  Address: {}\n",
            node.file_name().unwrap().to_string_lossy(),
            link("driver").unwrap_or_else(|| "unknown".into()),
            read("vendor"),
            read("device"),
            address.map_or("unknown".into(), |a| format!("PCI {a}"))
        );
    }
    out
}

fn combined_info() -> String {
    let file = |p: &str| std::fs::read_to_string(p).map_or("FAILED TO OPEN\n".to_string(), |t| format!("\n{t}"));
    let env: String = std::env::vars()
        .filter(|(k, _)| ["QS_", "QT_", "QML_", "QML2_", "QSG_", "QSRS_"].iter().any(|p| k.starts_with(p)) || k == "XDG_CURRENT_DESKTOP")
        .map(|(k, v)| format!("{k}={v}\n"))
        .collect();
    format!(
        "===== Version Information =====\nQuickshell: {}\nQt: {} (built against {})\n\n\
         ===== Build Information =====\nBuild Type: {}\nCompiler: {}\nCompile Flags: {}\n\n\
         ===== System Information =====\n{}\n/etc/os-release:{}/etc/lsb-release:{}\n\
         ===== Environment (trimmed) =====\n{env}",
        version(),
        quickshell::util::qt_version(),
        crate::host::ffi::qt_build_version(),
        if cfg!(debug_assertions) { "Debug" } else { "Release" },
        env!("QSRS_RUSTC"),
        env!("QSRS_RUSTFLAGS"),
        gpu_info(),
        file("/etc/os-release"),
        file("/etc/lsb-release"),
    )
}

pub fn print_version(verbosity: u32) -> i32 {
    if verbosity == 0 {
        println!("Quickshell {}", version());
    } else {
        print!("{}", combined_info());
    }
    0
}

/// Upstream's `--private-check-compat`: the Qt the binary runs on must be the one it was built for.
pub fn qt_mismatch() -> Option<(String, String)> {
    let (running, built) = (quickshell::util::qt_version().to_string(), crate::host::ffi::qt_build_version());
    (running != built).then_some((running, built))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, launch: i64) -> InstanceInfo {
        InstanceInfo {
            instance_id: id.into(),
            config_path: "/home/u/.config/quickshell/inir/shell.qml".into(),
            shell_id: "inir".into(),
            app_id: "org.quickshell".into(),
            launch_time_ms: launch,
            pid: 4242,
            display: "wayland/wayland-1".into(),
        }
    }

    #[test]
    fn json_matches_qjsondocument() {
        let s = list_json(&[(info("abc", 0), false), (info("d\"e", 0), true)]);
        assert!(s.starts_with("[\n    {\n        \"config_path\": \"/home/u/.config/quickshell/inir/shell.qml\",\n        \"id\": \"abc\",\n"));
        assert!(s.contains("    },\n    {\n"));
        assert!(s.contains("\"id\": \"d\\\"e\""));
        assert!(s.ends_with("        \"shell_id\": \"inir\"\n    }\n]\n"));
    }

    #[test]
    fn text_matches_upstream() {
        let launch = 1_000_000_000_000;
        let s = list_text(&[(info("abc", launch), false)], true, launch + (3600 + 2 * 60 + 5) * 1000 + 999);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "Instance abc:");
        assert_eq!(lines[1], "  Process ID: 4242");
        assert_eq!(lines[2], "  Shell ID: inir");
        assert_eq!(lines[3], "  Config path: /home/u/.config/quickshell/inir/shell.qml");
        assert_eq!(lines[4], "  Display connection: wayland/wayland-1");
        assert!(lines[5].starts_with("  Launch time: 2001-09-"));
        assert!(lines[5].ends_with(" (running for 1 hours, 2 minutes, 5 seconds)"));
        assert_eq!(lines[6], "");
        let dead = list_text(&[(info("abc", launch), true)], true, launch);
        assert!(dead.starts_with("\x1b[90mInstance abc (dead):\n") && dead.ends_with("\x1b[0m\n"));
        assert!(!dead.contains("running for"));
    }

    #[test]
    fn version_second_word_is_upstream() {
        let line = format!("Quickshell {}", version());
        assert_eq!(line.split_whitespace().nth(1), Some(UPSTREAM_VERSION));
    }

    #[test]
    fn sort_oldest_first_unless_newest() {
        let mut l = vec![info("b", 2), info("a", 1), info("c", 3)];
        sort(&mut l, false);
        assert_eq!(l.iter().map(|i| i.instance_id.as_str()).collect::<String>(), "abc");
        sort(&mut l, true);
        assert_eq!(l.iter().map(|i| i.instance_id.as_str()).collect::<String>(), "cba");
    }
}
