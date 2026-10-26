//! The `qs` command line (upstream launch/parsecommand.cpp, built on CLI11): same options,
//! subcommands, environment fallbacks and exclusions. Options of an enclosing command are also
//! accepted after a subcommand (`qs ipc call -c inir t f`), where CLI11 would reject them.

#[derive(Debug, Default, PartialEq)]
pub struct Cli {
    pub path: Option<String>,
    pub name: Option<String>,
    pub manifest: Option<String>,
    pub newest: bool,
    pub any_display: bool,
    pub no_color: bool,
    pub log_times: bool,
    pub log_rules: String,
    pub verbosity: u32,
    pub sparse: bool,
    pub debug_port: Option<u16>,
    pub wait_debug: bool,
    pub version: bool,
    pub no_duplicate: bool,
    pub daemonize: bool,
    pub check_compat: bool,
    pub id: Option<String>,
    pub pid: Option<i32>,
    pub all: bool,
    pub json: bool,
    pub show_dead: bool,
    pub cmd: Command,
}

#[derive(Debug, Default, PartialEq)]
pub enum Command {
    #[default]
    Launch,
    /// qs-rs only: compile every QML file of the shell and report errors.
    Check,
    /// qs-rs only: every warning and error of the running instance, grouped, plus `--static`.
    Errors { follow: bool, static_check: bool },
    /// qs-rs only: the running instance's frame meter (start [FILE], stop, stats, reset).
    Frames { action: String, arg: String },
    Log { file: Option<String>, tail: usize, follow: bool, rules: String },
    List,
    Kill,
    Ipc(Ipc),
}

#[derive(Debug, PartialEq)]
pub enum Ipc {
    Show { target: String, name: String },
    Call { target: String, function: String, args: Vec<String> },
    Wait { target: String, signal: String },
    Listen { target: String, signal: String },
    PropGet { target: String, property: String },
}

/// A parse result that ends the process: help text (code 0) or an error, with CLI11's codes.
#[derive(Debug, PartialEq)]
pub struct Exit {
    pub code: i32,
    pub text: String,
}

const EXTRAS: i32 = 109;
const EXCLUDES: i32 = 108;
const CONVERSION: i32 = 104;
const VALIDATION: i32 = 105;
const REQUIRED: i32 = 106;
const ARG_MISMATCH: i32 = 112;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Level {
    Root,
    Check,
    Errors,
    Frames,
    Memory,
    Ui,
    Log,
    List,
    Kill,
    Ipc,
    IpcShow,
    IpcCall,
    IpcWait,
    IpcListen,
    Prop,
    PropGet,
    Msg,
}

impl Level {
    fn parent(self) -> Option<Level> {
        use Level::*;
        match self {
            Root => None,
            Check | Errors | Frames | Memory | Ui | Log | List | Kill | Ipc | Msg => Some(Root),
            IpcShow | IpcCall | IpcWait | IpcListen | Prop => Some(Ipc),
            PropGet => Some(Prop),
        }
    }

    fn name(self) -> &'static str {
        use Level::*;
        match self {
            Root => "qs",
            Check => "check",
            Errors => "errors",
            Frames => "frames",
            Memory => "memory",
            Ui => "ui",
            Log => "log",
            List => "list",
            Kill => "kill",
            Ipc => "ipc",
            IpcShow => "show",
            IpcCall => "call",
            IpcWait => "wait",
            IpcListen => "listen",
            Prop => "prop",
            PropGet => "get",
            Msg => "msg",
        }
    }

    fn subcommand(self, word: &str) -> Option<Level> {
        use Level::*;
        Some(match (self, word) {
            (Root, "check") => Check,
            (Root, "errors") => Errors,
            (Root, "frames") => Frames,
            (Root, "memory") => Memory,
            (Root, "ui") => Ui,
            (Root, "log") => Log,
            (Root, "list") => List,
            (Root, "kill") => Kill,
            (Root, "ipc") => Ipc,
            (Root, "msg") => Msg,
            (Ipc, "show") => IpcShow,
            (Ipc, "call") => IpcCall,
            (Ipc, "wait") => IpcWait,
            (Ipc, "listen") => IpcListen,
            (Ipc, "prop") => Prop,
            (Prop, "get") => PropGet,
            _ => return None,
        })
    }

    /// Commands that take the instance filters (-n is --newest there, --no-duplicate at the root).
    fn filtering(self) -> bool {
        matches!(self, Level::Log | Level::List | Level::Kill | Level::Ipc | Level::Msg)
    }

    fn instance_selection(self) -> bool {
        matches!(self, Level::Log | Level::Errors | Level::Frames | Level::Memory | Level::Ui | Level::Kill | Level::Ipc | Level::Msg)
    }

    fn logging(self) -> bool {
        matches!(self, Level::Root | Level::Check | Level::Errors | Level::Frames | Level::Memory | Level::Ui | Level::Log | Level::List | Level::Kill | Level::Ipc | Level::Msg)
    }

    fn config_selection(self) -> bool {
        self.logging()
    }
}

#[derive(Default)]
struct Seen {
    path: bool,
    name: bool,
    manifest: bool,
    all: bool,
    instance: Option<&'static str>,
    config: Option<&'static str>,
    file: bool,
}

struct Parser {
    cli: Cli,
    seen: Seen,
    /// Deepest command entered, and the positionals given to it.
    level: Level,
    positionals: Vec<String>,
    msg_show: bool,
    log_tail: usize,
    log_follow: bool,
    log_rules: String,
    static_check: bool,
}

fn err(code: i32, text: impl Into<String>) -> Exit {
    Exit { code, text: format!("{}\nRun with --help for more information.", text.into()) }
}

impl Parser {
    /// Applies option `name` (`-x` or `--long`) at `level`; Ok(false) when `level` lacks it.
    fn option(&mut self, level: Level, name: &str, value: &mut dyn FnMut() -> Option<String>) -> Result<bool, Exit> {
        let mut need = |name: &str| value().ok_or_else(|| err(ARG_MISMATCH, format!("{name}: 1 required TEXT missing")));
        let c = &mut self.cli;
        match name {
            "-h" | "--help" => return Err(Exit { code: 0, text: help(level) }),
            "-p" | "--path" if level.config_selection() => {
                c.path = Some(need(name)?);
                self.seen.path = true;
                self.seen.config.get_or_insert("--path");
            }
            "-c" | "--config" if level.config_selection() => {
                c.name = Some(need(name)?);
                self.seen.name = true;
                self.seen.config.get_or_insert("--config");
            }
            "-m" | "--manifest" if level.config_selection() => {
                c.manifest = Some(need(name)?);
                self.seen.manifest = true;
                self.seen.config.get_or_insert("--manifest");
            }
            "-n" | "--newest" if level.filtering() => {
                c.newest = true;
                self.seen.config.get_or_insert("--newest");
            }
            "--any-display" if level.filtering() => {
                c.any_display = true;
                self.seen.config.get_or_insert("--any-display");
            }
            "--no-color" if level.logging() => c.no_color = true,
            "--log-times" if level.logging() => c.log_times = true,
            "--log-rules" if level.logging() => c.log_rules = need(name)?,
            "-v" | "--verbose" if level.logging() => c.verbosity += 1,
            "--no-detailed-logs" if level.logging() => c.sparse = true,
            "--debug" if level == Level::Root => {
                let v = need(name)?;
                let port = v.parse::<i64>().map_err(|_| err(CONVERSION, format!("The value {v} is not an allowed value for {name}")))?;
                c.debug_port = Some(u16::try_from(port).map_err(|_| err(VALIDATION, format!("{name}: Value {v} not in range [0 - 65535]")))?);
            }
            "--waitfordebug" if level == Level::Root => c.wait_debug = true,
            "--private-check-compat" if level == Level::Root => c.check_compat = true,
            "-V" | "--version" if level == Level::Root => c.version = true,
            "-n" | "--no-duplicate" if level == Level::Root => c.no_duplicate = true,
            "-d" | "--daemonize" if level == Level::Root => c.daemonize = true,
            "-i" | "--id" if level.instance_selection() => {
                c.id = Some(need(name)?);
                self.seen.instance.get_or_insert("--id");
            }
            "--pid" if level.instance_selection() => {
                let v = need(name)?;
                c.pid = Some(v.parse().map_err(|_| err(CONVERSION, format!("The value {v} is not an allowed value for --pid")))?);
                self.seen.instance.get_or_insert("--pid");
            }
            "-a" | "--all" if level == Level::List => {
                c.all = true;
                self.seen.all = true;
            }
            "-j" | "--json" if level == Level::List => c.json = true,
            "--show-dead" if level == Level::List => c.show_dead = true,
            "-t" | "--tail" if level == Level::Log => {
                let v = need(name)?;
                self.log_tail = match v.parse::<i64>() {
                    Ok(n) if n >= 1 && n <= i32::MAX as i64 => n as usize,
                    Ok(_) => return Err(err(VALIDATION, format!("{name}: Value {v} not in range INT > 0"))),
                    Err(_) => return Err(err(CONVERSION, format!("The value {v} is not an allowed value for {name}"))),
                };
            }
            "-f" | "--follow" if level == Level::Log || level == Level::Errors => self.log_follow = true,
            "--static" if level == Level::Errors => self.static_check = true,
            "-r" | "--rules" if level == Level::Log => self.log_rules = need(name)?,
            "-s" | "--show" if level == Level::Msg => self.msg_show = true,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Looks the option up in the current command, then in the commands around it.
    fn apply(&mut self, name: &str, value: &mut dyn FnMut() -> Option<String>) -> Result<(), Exit> {
        let mut level = Some(self.level);
        while let Some(l) = level {
            if self.option(l, name, value)? {
                return Ok(());
            }
            level = l.parent();
        }
        Err(err(EXTRAS, format!("{}: The following argument was not expected: {name}", self.level.name())))
    }

    fn positional(&mut self, word: String) -> Result<(), Exit> {
        if self.positionals.is_empty()
            && let Some(sub) = self.level.subcommand(&word) {
                self.level = sub;
                return Ok(());
            }
        // qs-rs extension: `qs-rs file.qml` runs that file, as the QML tests do.
        if self.level == Level::Root && !self.seen.path && word.ends_with(".qml") {
            self.cli.path = Some(word);
            self.seen.path = true;
            self.seen.config.get_or_insert("--path");
            return Ok(());
        }
        let max = match self.level {
            Level::Log | Level::Memory => 1,
            Level::IpcCall | Level::Msg | Level::Ui => usize::MAX,
            Level::IpcWait | Level::IpcListen | Level::PropGet | Level::Frames => 2,
            _ => 0,
        };
        if self.positionals.len() >= max {
            return Err(err(EXTRAS, format!("The following argument was not expected: {word}")));
        }
        if self.level == Level::Log {
            self.seen.file = true;
        }
        self.positionals.push(word);
        Ok(())
    }

    fn finish(mut self) -> Result<Cli, Exit> {
        let s = &self.seen;
        if s.path && s.name {
            return Err(err(EXCLUDES, "--path excludes --config"));
        }
        if s.path && s.manifest {
            return Err(err(EXCLUDES, "--path excludes --manifest"));
        }
        if let (Some(i), Some(c)) = (s.instance, s.config) {
            return Err(err(EXCLUDES, format!("{i} excludes {c}")));
        }
        if s.all
            && let Some(c) = s.config.filter(|c| ["--path", "--config", "--manifest"].contains(c)) {
                return Err(err(EXCLUDES, format!("--all excludes {c}")));
            }
        if s.file && (s.instance.is_some() || s.config.is_some()) {
            return Err(err(EXCLUDES, "file excludes instance and config selection"));
        }
        if self.cli.wait_debug && self.cli.debug_port.is_none() {
            return Err(err(REQUIRED, "--waitfordebug requires --debug"));
        }
        // CLI11 envname: only for options not given on the command line.
        let env = |k| std::env::var(k).ok().filter(|v| !v.is_empty());
        let given = s.path || s.name || s.manifest;
        if !given && !s.all && s.instance.is_none() && !s.file {
            if let Some(p) = env("QS_CONFIG_PATH") {
                self.cli.path = Some(p);
            } else {
                self.cli.name = env("QS_CONFIG_NAME");
                self.cli.manifest = env("QS_MANIFEST");
            }
        }
        let mut pos = std::mem::take(&mut self.positionals).into_iter();
        let mut next = || pos.next().unwrap_or_default();
        self.cli.cmd = match self.level {
            Level::Root => Command::Launch,
            Level::Check => Command::Check,
            Level::Errors => Command::Errors { follow: self.log_follow, static_check: self.static_check },
            Level::Frames => Command::Frames { action: Some(next()).filter(|a| !a.is_empty()).unwrap_or_else(|| "stats".into()), arg: next() },
            // Answered by the same runtime hook as the frame meter.
            Level::Memory => match next().as_str() {
                "" => Command::Frames { action: "memory".into(), arg: String::new() },
                "reclaim" => Command::Frames { action: "memory".into(), arg: "reclaim".into() },
                other => return Err(err(VALIDATION, format!("Unknown memory action: {other} (expected reclaim)"))),
            },
            // Answered by the same runtime hook as the frame meter.
            Level::Ui => Command::Frames { action: "ui".into(), arg: pos.collect::<Vec<_>>().join(" ") },
            Level::Log => Command::Log {
                file: Some(next()).filter(|f| !f.is_empty()),
                tail: self.log_tail,
                follow: self.log_follow,
                rules: self.log_rules,
            },
            Level::List => Command::List,
            Level::Kill => Command::Kill,
            Level::Ipc | Level::Prop => return Err(err(REQUIRED, "A subcommand is required")),
            Level::IpcShow => Command::Ipc(Ipc::Show { target: String::new(), name: String::new() }),
            Level::IpcCall => Command::Ipc(Ipc::Call { target: next(), function: next(), args: pos.collect() }),
            Level::IpcWait => Command::Ipc(Ipc::Wait { target: next(), signal: next() }),
            Level::IpcListen => Command::Ipc(Ipc::Listen { target: next(), signal: next() }),
            Level::PropGet => Command::Ipc(Ipc::PropGet { target: next(), property: next() }),
            Level::Msg if self.msg_show => Command::Ipc(Ipc::Show { target: next(), name: next() }),
            Level::Msg => Command::Ipc(Ipc::Call { target: next(), function: next(), args: pos.collect() }),
        };
        Ok(self.cli)
    }
}

/// Short options that take a value; a cluster ends at the first of them (`-ndp DIR`, `-pDIR`).
const SHORT_VALUES: &[&str] = &["-p", "-c", "-m", "-i", "-t", "-r"];

/// A token that CLI11 would read as a positional although it starts with '-': a number.
fn is_number(s: &str) -> bool {
    s.len() > 1 && s.parse::<f64>().is_ok()
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Cli, Exit> {
    let mut p = Parser {
        cli: Cli::default(),
        seen: Seen::default(),
        level: Level::Root,
        positionals: vec![],
        msg_show: false,
        log_tail: 0,
        log_follow: false,
        log_rules: String::new(),
        static_check: false,
    };
    let mut it = args.into_iter().peekable();
    while let Some(tok) = it.next() {
        if tok == "--" {
            for rest in it.by_ref() {
                p.positional(rest)?;
            }
            break;
        }
        if let Some(long) = tok.strip_prefix("--").filter(|l| !l.is_empty()) {
            let (name, mut inline) = match long.split_once('=') {
                Some((n, v)) => (format!("--{n}"), Some(v.to_string())),
                None => (tok.clone(), None),
            };
            let mut value = || inline.take().or_else(|| it.next());
            p.apply(&name, &mut value)?;
        } else if tok.len() > 1 && tok.starts_with('-') && !is_number(&tok) {
            // A cluster of short flags, the last one may take a value (`-vv`, `-nd`, `-pDIR`).
            let chars: Vec<char> = tok[1..].chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let name = format!("-{}", chars[i]);
                if SHORT_VALUES.contains(&name.as_str()) {
                    let attached: String = chars[i + 1..].iter().collect();
                    let mut inline = (!attached.is_empty()).then_some(attached);
                    let mut value = || inline.take().or_else(|| it.next());
                    p.apply(&name, &mut value)?;
                    break;
                }
                p.apply(&name, &mut || None)?;
                i += 1;
            }
        } else {
            p.positional(tok)?;
        }
    }
    if p.level == Level::Msg && !p.msg_show && p.positionals.is_empty() {
        return Err(err(REQUIRED, "msg: At least 1 option required"));
    }
    p.finish()
}

fn help(level: Level) -> String {
    const CONFIG: &str = "Config selection:\n  \
        -p, --path TEXT         Path to a QML file or config folder. (env QS_CONFIG_PATH)\n  \
        -c, --config TEXT       Name of a configuration under <xdg config dir>/quickshell/. (env QS_CONFIG_NAME)\n  \
        -m, --manifest TEXT     [DEPRECATED] Path to a quickshell manifest. (env QS_MANIFEST)\n";
    const FILTER: &str = "  -n, --newest            Operate on the most recently launched instance instead of the oldest.\n      \
        --any-display       Do not filter instances by the display they were launched on.\n";
    const INSTANCE: &str = "Instance selection:\n  \
        -i, --id TEXT           Instance id to operate on (a unique prefix is enough).\n      \
        --pid INT           Process id of the instance to operate on.\n";
    const LOGGING: &str = "Logging:\n      \
        --no-color          Disable colored logging (also a non-empty NO_COLOR).\n      \
        --log-times         Log timestamps with each message.\n      \
        --log-rules TEXT    Log rules to apply, in the format of QT_LOGGING_RULES.\n  \
        -v, --verbose           -v shows INFO, -vv DEBUG internal logs.\n";
    
    match level {
        Level::Root => format!(
            "qs [OPTIONS] [SUBCOMMAND]\n\nOptions:\n  \
             -h, --help              Print this help message and exit.\n  \
             -V, --version           Print the version and exit (-v for build information).\n  \
             -n, --no-duplicate      Exit immediately if another instance of the given config is running.\n  \
             -d, --daemonize         Detach from the controlling terminal.\n      \
             --debug INT         Open the given port for a QML debugger connection.\n      \
             --waitfordebug      Wait for a QML debugger before running the configuration.\n\n\
             {CONFIG}\n{LOGGING}\nSubcommands:\n  \
             log                     Print quickshell logs.\n  \
             list                    List running quickshell instances.\n  \
             kill                    Kill quickshell instances.\n  \
             ipc                     Communicate with other Quickshell instances.\n  \
             check                   Compile every QML file of the config and report errors (qs-rs).\n  \
             errors                  Warnings and errors of a running instance, grouped (qs-rs).\n  \
             memory                  Where a running instance's memory sits (qs-rs).\n  \
             ui                      What a person would read on screen, and where (qs-rs).\n  \
             msg                     [DEPRECATED] Moved to `ipc call`.\n"
        ),
        Level::Log => format!(
            "qs log [OPTIONS] [file]\n\n  \
             file                    Log file to read.\n  \
             -t, --tail INT          Maximum number of lines to print, starting from the bottom.\n  \
             -f, --follow            Keep reading until the logging process terminates.\n  \
             -r, --rules TEXT        Rules to apply to the log being read (QT_LOGGING_RULES format).\n\n\
             {INSTANCE}\n{CONFIG}{FILTER}\n{LOGGING}"
        ),
        Level::List => format!(
            "qs list [OPTIONS]\n\n  \
             -a, --all               List all instances, not only those of the selected config.\n  \
             -j, --json              Output the list as json.\n      \
             --show-dead         Include dead instances in the list.\n\n{CONFIG}{FILTER}"
        ),
        Level::Kill => format!("qs kill [OPTIONS]\n\n{INSTANCE}\n{CONFIG}{FILTER}"),
        Level::Ipc | Level::IpcShow | Level::IpcCall | Level::IpcWait | Level::IpcListen | Level::Prop | Level::PropGet => format!(
            "qs ipc [OPTIONS] SUBCOMMAND\n\nSubcommands:\n  \
             show                    Print information about available IPC targets.\n  \
             call TARGET FUNCTION [ARGS...]   Call an IpcHandler function.\n  \
             wait TARGET SIGNAL      Wait for one IpcHandler signal.\n  \
             listen TARGET SIGNAL    Listen for IpcHandler signals.\n  \
             prop get TARGET PROPERTY   Read the value of a property.\n\n{INSTANCE}\n{CONFIG}{FILTER}"
        ),
        Level::Msg => format!("qs msg [OPTIONS] TARGET FUNCTION [ARGS...]\n\n  -s, --show              Print information about a function or target.\n\n{INSTANCE}\n{CONFIG}{FILTER}"),
        Level::Check => format!("qs check [OPTIONS]\n\n{CONFIG}"),
        Level::Memory => format!(
            "qs memory [OPTIONS] [reclaim]\n\n  \
             Where the instance's memory sits: process and allocator, live objects by the file\n  \
             that made them, and the images and offscreen layers they hold (upper bounds).\n\n  \
             reclaim                 Run a reclaim pass first (JS GC, unused components, purge).\n\n\
             {INSTANCE}\n{CONFIG}{FILTER}"
        ),
        Level::Ui => format!(
            "qs ui [OPTIONS] [in:WINDOW] [windows | find WORDS | at X Y | textures]\n\n  \
             What a person would read on screen: every visible item in screen pixels, minus what\n  \
             is hidden, see-through or clipped away. One line per item: centre, rect, window,\n  \
             type, text, flags (t, hover, pressed, focus, off, cut, over, part, edge, glyph).\n\n  \
             (nothing)               Every visible text.\n  \
             windows                 The shown windows and where they sit.\n  \
             find WORDS              Items whose text or objectName holds WORDS.\n  \
             at X Y                  Every item under a point, bottom to top.\n  \
             textures                Images asking one picture with and without mipmaps.\n  \
             in:WINDOW               Only windows whose name starts so (layer namespace or file).\n\n\
             {INSTANCE}\n{CONFIG}{FILTER}"
        ),
        Level::Frames => format!(
            "qs frames [OPTIONS] [ACTION [FILE]]\n\n  \
             The instance's frame meter: per window, frame cost and its stages (polish, sync,\n  \
             render, swap), refreshes missed and animation steps per frame.\n\n  \
             start [FILE]            Turn it on (FILE gets one line per frame and window).\n  \
             stats                   Print what it measured so far, as json (the default).\n  \
             reset                   Start counting again.\n  \
             stop                    Print the stats and turn it off.\n\n\
             {INSTANCE}\n{CONFIG}{FILTER}"
        ),
        Level::Errors => format!(
            "qs errors [OPTIONS]\n\n  \
             Every warning and error the instance logged since it started, grouped by kind with\n  \
             counts and the side that owns it (the shell by area, the runtime, the system).\n\n  \
             -f, --follow            Keep reading and print each new kind as it first shows up.\n      \
             --static            Also compile every QML file of the config (all families, nothing shown).\n\n\
             {INSTANCE}\n{CONFIG}{FILTER}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &str) -> Result<Cli, Exit> {
        parse(args.split_whitespace().map(String::from))
    }

    #[test]
    fn inir_calls() {
        let c = p("-n -d -p /x/inir").unwrap();
        assert!(c.no_duplicate && c.daemonize && c.cmd == Command::Launch);
        assert_eq!(c.path.as_deref(), Some("/x/inir"));

        let c = p("list --all --json").unwrap();
        assert!(c.all && c.json && c.cmd == Command::List);

        let c = p("-p /x list --any-display").unwrap();
        assert!(c.any_display && !c.all);

        let c = p("ipc --pid 42 call dev open settings").unwrap();
        assert_eq!(c.pid, Some(42));
        assert_eq!(c.cmd, Command::Ipc(Ipc::Call { target: "dev".into(), function: "open".into(), args: vec!["settings".into()] }));

        assert_eq!(p("ipc --pid 42 show").unwrap().cmd, Command::Ipc(Ipc::Show { target: String::new(), name: String::new() }));
        assert_eq!(p("-p /x kill").unwrap().cmd, Command::Kill);
        assert!(p("--version").unwrap().version);
        let c = p("-p /x log --rules *=true").unwrap();
        assert_eq!(c.cmd, Command::Log { file: None, tail: 0, follow: false, rules: "*=true".into() });
    }

    #[test]
    fn n_depends_on_the_command() {
        let root = p("-n -p /x").unwrap();
        assert!(root.no_duplicate && !root.newest);
        let sub = p("ipc -n -p /x show").unwrap();
        assert!(sub.newest && !sub.no_duplicate);
    }

    #[test]
    fn value_forms_and_clusters() {
        assert_eq!(p("--path=/a").unwrap().path.as_deref(), Some("/a"));
        assert_eq!(p("-p/a").unwrap().path.as_deref(), Some("/a"));
        assert_eq!(p("-vv").unwrap().verbosity, 2);
        let c = p("-ndp /a").unwrap();
        assert!(c.no_duplicate && c.daemonize && c.path.as_deref() == Some("/a"));
        assert_eq!(p("errors --static -f").unwrap().cmd, Command::Errors { follow: true, static_check: true });
        assert_eq!(p("errors").unwrap().cmd, Command::Errors { follow: false, static_check: false });
        assert_eq!(p("memory").unwrap().cmd, Command::Frames { action: "memory".into(), arg: String::new() });
        assert_eq!(p("memory reclaim").unwrap().cmd, Command::Frames { action: "memory".into(), arg: "reclaim".into() });
        assert_eq!(p("ui in:quickshell:iris find Glass").unwrap().cmd, Command::Frames { action: "ui".into(), arg: "in:quickshell:iris find Glass".into() });
        assert_eq!(p("ui").unwrap().cmd, Command::Frames { action: "ui".into(), arg: String::new() });
        assert!(p("memory everything").is_err());
        assert!(p("check --static").is_err());
        assert_eq!(p("log -t 5 -f").unwrap().cmd, Command::Log { file: None, tail: 5, follow: true, rules: String::new() });
    }

    #[test]
    fn negative_numbers_are_arguments() {
        let c = p("ipc call audio step -5").unwrap();
        assert_eq!(c.cmd, Command::Ipc(Ipc::Call { target: "audio".into(), function: "step".into(), args: vec!["-5".into()] }));
    }

    #[test]
    fn parent_options_after_subcommand() {
        let c = p("ipc call -c inir t f").unwrap();
        assert_eq!(c.name.as_deref(), Some("inir"));
    }

    #[test]
    fn errors_like_cli11() {
        assert_eq!(p("list --bogus").unwrap_err().code, EXTRAS);
        assert_eq!(p("bogus").unwrap_err().code, EXTRAS);
        assert_eq!(p("-p /a -c b").unwrap_err().code, EXCLUDES);
        assert_eq!(p("kill --pid 3 -p /a").unwrap_err().code, EXCLUDES);
        assert_eq!(p("list -a -c x").unwrap_err().code, EXCLUDES);
        assert_eq!(p("ipc").unwrap_err().code, REQUIRED);
        assert_eq!(p("msg").unwrap_err().code, REQUIRED);
        assert_eq!(p("--pid 3").unwrap_err().code, EXTRAS);
        assert_eq!(p("log -t 0").unwrap_err().code, VALIDATION);
        assert_eq!(p("--waitfordebug").unwrap_err().code, REQUIRED);
        assert_eq!(p("-h").unwrap_err().code, 0);
    }

    #[test]
    fn msg_maps_to_ipc() {
        assert_eq!(p("msg t f a").unwrap().cmd, Command::Ipc(Ipc::Call { target: "t".into(), function: "f".into(), args: vec!["a".into()] }));
        assert_eq!(p("msg -s t").unwrap().cmd, Command::Ipc(Ipc::Show { target: "t".into(), name: String::new() }));
    }
}
