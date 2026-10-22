//! Instance logging like upstream core/logging.cpp: every message (Qt's through the handler in
//! include/log.h, Rust's through `qs_log!`) goes to stdout when its category allows it, and to
//! `by-id/<id>/log.log` (what was shown) and `log.qslog` (everything, upstream's binary format
//! v2, which `qs log` and `qs-rs log` read). Writes happen on the calling thread under one lock;
//! the files live on tmpfs, so there is no logging thread.

#[cxx_qt::bridge]
pub mod ffi {
    extern "Rust" {
        /// A Qt message (QtMsgType values) from the C++ message handler.
        fn log_qt_message(kind: i32, category: &str, body: &str);
    }
}

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Critical,
    Fatal,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Message {
    pub level: Level,
    pub category: String,
    pub body: String,
    /// Milliseconds since the epoch.
    pub time: i64,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// `yyyy-MM-dd hh:mm:ss.zzz` in local time.
fn local_time(ms: i64) -> String {
    let secs = ms.div_euclid(1000) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        ms.rem_euclid(1000)
    )
}

/// One line the way upstream's LogMessage::formatMessage writes it.
pub fn format(msg: &Message, color: bool, timestamp: bool, prefix: &str) -> String {
    let mut s = String::new();
    if !prefix.is_empty() {
        if color {
            s.push_str("\x1b[90m");
        }
        s.push('[');
        s.push_str(prefix);
        s.push(']');
        if timestamp {
            s.push(' ');
        }
        if color {
            s.push_str("\x1b[0m");
        }
    }
    if timestamp {
        if color {
            s.push_str("\x1b[90m");
        }
        s.push_str(&local_time(msg.time));
    }
    if msg.category == "quickshell.bare" {
        if !prefix.is_empty() {
            s.push(' ');
        }
        s.push_str(&msg.body);
        return s;
    }
    let label = match msg.level {
        Level::Debug => (" DEBUG", "\x1b[34m"),
        Level::Info => ("  INFO", "\x1b[32m"),
        Level::Warn => ("  WARN", "\x1b[33m"),
        Level::Critical => (" ERROR", "\x1b[31m"),
        Level::Fatal => (" FATAL", "\x1b[31m"),
    };
    if color {
        s.push_str(label.1);
    }
    s.push_str(label.0);
    let is_default = msg.category == "default";
    let fatal = msg.level == Level::Fatal;
    if color && !is_default && !fatal {
        s.push_str("\x1b[97m");
    }
    if !is_default {
        s.push(' ');
        s.push_str(&msg.category);
    }
    if color && !fatal {
        s.push_str("\x1b[0m");
    }
    s.push_str(": ");
    s.push_str(&msg.body);
    if color && fatal {
        s.push_str("\x1b[0m");
    }
    s
}

// ---- rules ----

/// Which levels of a category show (upstream CategoryFilter).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Filter {
    pub debug: bool,
    pub info: bool,
    pub warn: bool,
    pub critical: bool,
}

impl Filter {
    pub fn from_level(min: Level) -> Self {
        Filter { debug: min <= Level::Debug, info: min <= Level::Info, warn: min <= Level::Warn, critical: true }
    }

    pub fn shows(&self, level: Level) -> bool {
        match level {
            Level::Debug => self.debug,
            Level::Info => self.info,
            Level::Warn => self.warn,
            Level::Critical | Level::Fatal => self.critical,
        }
    }

    fn flags(&self) -> u8 {
        self.debug as u8 | (self.info as u8) << 1 | (self.warn as u8) << 2 | (self.critical as u8) << 3
    }

    fn from_flags(f: u8) -> Self {
        Filter { debug: f & 1 != 0, info: f & 2 != 0, warn: f & 4 != 0, critical: f & 8 != 0 }
    }
}

/// One `category[.level]=bool` rule with Qt's leading/trailing `*` wildcards.
#[derive(Clone, Debug)]
pub struct Rule {
    pattern: String,
    level: Option<Level>,
    enabled: bool,
}

/// Parses QT_LOGGING_RULES syntax (`;` or newline separated).
pub fn parse_rules(text: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    for line in text.split([';', '\n']) {
        let Some((key, value)) = line.trim().split_once('=') else { continue };
        let enabled = match value.trim() {
            "true" => true,
            "false" => false,
            _ => continue,
        };
        let key = key.trim();
        let (pattern, level) = match key.rsplit_once('.') {
            Some((p, "debug")) => (p, Some(Level::Debug)),
            Some((p, "info")) => (p, Some(Level::Info)),
            Some((p, "warning")) => (p, Some(Level::Warn)),
            Some((p, "critical")) => (p, Some(Level::Critical)),
            _ => (key, None),
        };
        rules.push(Rule { pattern: pattern.to_string(), level, enabled });
    }
    rules
}

fn matches(pattern: &str, category: &str) -> bool {
    match (pattern.strip_prefix('*'), pattern.strip_suffix('*')) {
        _ if pattern == "*" => true,
        (Some(rest), _) if rest.ends_with('*') => category.contains(&rest[..rest.len() - 1]),
        (Some(rest), _) => category.ends_with(rest),
        (None, Some(rest)) => category.starts_with(rest),
        (None, None) => category == pattern,
    }
}

/// `base` with every matching rule applied in order (later rules win, like Qt).
pub fn apply_rules(rules: &[Rule], category: &str, mut base: Filter) -> Filter {
    for r in rules.iter().filter(|r| matches(&r.pattern, category)) {
        match r.level {
            None => {
                base.debug = r.enabled;
                base.info = r.enabled;
                base.warn = r.enabled;
                base.critical = r.enabled;
            }
            Some(Level::Debug) => base.debug = r.enabled,
            Some(Level::Info) => base.info = r.enabled,
            Some(Level::Warn) => base.warn = r.enabled,
            Some(_) => base.critical = r.enabled,
        }
    }
    base
}

/// A category's filter before rules (upstream LogManager::filterCategory): `quickshell*`
/// categories start at the default level (`-v`), others at Qt's defaults (all on, but debug off
/// for `qt.*`).
pub fn base_filter(category: &str, default_level: Level) -> Filter {
    if category.starts_with("quickshell") {
        Filter::from_level(default_level)
    } else {
        Filter { debug: !category.starts_with("qt."), info: true, warn: true, critical: true }
    }
}

// ---- qslog encoding (upstream EncodedLogWriter/Reader, version 2) ----

const LOG_VERSION: u8 = 2;
const OP_REGISTER_CATEGORY: u8 = 0;
const OP_RECENT_SHORT: u8 = 1;
const OP_RECENT_LONG: u8 = 2;
const BEGIN_CATEGORIES: u32 = 3;
const RECENT: usize = 256;

fn compressed(level: Level) -> u8 {
    match level {
        Level::Debug => 0,
        Level::Info => 1,
        Level::Warn => 2,
        Level::Critical | Level::Fatal => 3,
    }
}

fn write_varint(out: &mut Vec<u8>, n: u32) {
    if n < 0xff {
        out.push(n as u8);
    } else if n < 0xffff {
        out.push(0xff);
        out.extend_from_slice(&(n as u16).to_le_bytes());
    } else {
        out.push(0xff);
        out.extend_from_slice(&0xffffu16.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
    }
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    write_varint(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}

/// Newest first, at most RECENT (upstream's HashBuffer/RingBuffer: index 0 is the last one).
#[derive(Default)]
struct Recent(std::collections::VecDeque<Message>);

impl Recent {
    fn index_of(&self, m: &Message) -> Option<usize> {
        self.0.iter().position(|r| r.level == m.level && r.category == m.category && r.body == m.body)
    }

    fn push(&mut self, m: Message) {
        if self.0.len() == RECENT {
            self.0.pop_back();
        }
        self.0.push_front(m);
    }
}

pub struct Encoder {
    categories: HashMap<String, u32>,
    recent: Recent,
    last_secs: i64,
}

impl Default for Encoder {
    fn default() -> Self {
        Self { categories: HashMap::new(), recent: Recent::default(), last_secs: 0 }
    }
}

impl Encoder {
    pub fn header() -> [u8; 1] {
        [LOG_VERSION]
    }

    pub fn encode(&mut self, m: &Message, filter: impl Fn(&str) -> Filter, out: &mut Vec<u8>) {
        let secs = m.time.div_euclid(1000);
        let delta = secs - self.last_secs;
        match self.recent.index_of(m) {
            Some(index) => {
                if (0..16).contains(&delta) && index < 16 {
                    out.push(OP_RECENT_SHORT);
                    out.push(index as u8 | (delta as u8) << 4);
                } else {
                    out.push(OP_RECENT_LONG);
                    out.push(index as u8);
                    write_varint(out, delta as u32);
                }
            }
            None => {
                let id = match self.categories.get(&m.category) {
                    Some(id) => *id,
                    None => {
                        out.push(OP_REGISTER_CATEGORY);
                        write_string(out, &m.category);
                        out.push(filter(&m.category).flags());
                        let id = BEGIN_CATEGORIES + self.categories.len() as u32;
                        self.categories.insert(m.category.clone(), id);
                        id
                    }
                };
                write_varint(out, id);
                if m.level == Level::Fatal {
                    out.push(0xff);
                    out.extend_from_slice(&(secs as u64).to_le_bytes());
                } else {
                    let mut field = compressed(m.level);
                    if delta >= 0x1d {
                        field |= (if delta < 0xffff { 0x1d } else { 0x1e }) << 3;
                    } else {
                        field |= (delta.max(0) as u8) << 3;
                    }
                    out.push(field);
                    if delta >= 0x1d {
                        if delta > 0xffff {
                            out.extend_from_slice(&(secs as u64).to_le_bytes());
                        } else {
                            write_varint(out, delta as u32);
                        }
                    }
                }
                write_string(out, &m.body);
            }
        }
        self.recent.push(m.clone());
        self.last_secs = secs;
    }
}

/// Reads messages back from a qslog byte stream.
pub struct Decoder {
    categories: Vec<(String, Filter)>,
    recent: Recent,
    last_secs: i64,
}

impl Default for Decoder {
    fn default() -> Self {
        Self { categories: Vec::new(), recent: Recent::default(), last_secs: 0 }
    }
}

struct Cursor<'a>(&'a [u8], usize);

impl Cursor<'_> {
    fn bytes(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.0.get(self.1..self.1 + n)?;
        self.1 += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.bytes(1).map(|b| b[0])
    }
    fn u64(&mut self) -> Option<u64> {
        self.bytes(8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    }
    fn varint(&mut self) -> Option<u32> {
        let rest = &self.0[self.1..];
        if rest.first().is_some_and(|b| *b != 0xff) {
            self.1 += 1;
            return Some(rest[0] as u32);
        }
        if rest.len() >= 3 && (rest[1] != 0xff || rest[2] != 0xff) {
            self.1 += 3;
            return Some(u16::from_le_bytes([rest[1], rest[2]]) as u32);
        }
        if rest.len() >= 7 {
            self.1 += 7;
            return Some(u32::from_le_bytes(rest[3..7].try_into().unwrap()));
        }
        None
    }
    fn string(&mut self) -> Option<String> {
        let n = self.varint()? as usize;
        self.bytes(n).map(|b| String::from_utf8_lossy(b).into_owned())
    }
}

impl Decoder {
    /// Checks the version byte; `Err` names the file's version.
    pub fn header(data: &[u8]) -> Result<usize, u8> {
        match data.first() {
            Some(&LOG_VERSION) => Ok(1),
            Some(v) => Err(*v),
            None => Ok(0),
        }
    }

    /// The next message at `pos` and the position after it, or `None` when the data ends inside
    /// a record (more may be written later).
    pub fn next(&mut self, data: &[u8], pos: usize) -> Option<(Message, Filter, usize)> {
        let mut c = Cursor(data, pos);
        loop {
            let op = c.varint()?;
            if op == OP_REGISTER_CATEGORY as u32 {
                let name = c.string()?;
                let flags = c.u8()?;
                self.categories.push((name, Filter::from_flags(flags)));
                continue;
            }
            let msg = if op == OP_RECENT_SHORT as u32 || op == OP_RECENT_LONG as u32 {
                let (index, delta) = if op == OP_RECENT_SHORT as u32 {
                    let f = c.u8()?;
                    ((f & 0xf) as usize, (f >> 4) as i64)
                } else {
                    (c.u8()? as usize, c.varint()? as i64)
                };
                let mut m = self.recent.0.get(index)?.clone();
                self.last_secs += delta;
                m.time = self.last_secs * 1000;
                m
            } else {
                let (category, _) = self.categories.get((op - BEGIN_CATEGORIES) as usize)?.clone();
                let field = c.u8()?;
                // The u64 after 0xff and 0x1e is an absolute epoch second (upstream's reader adds
                // it to the last time, which is only right at the start of a file).
                let level = if field == 0xff { Level::Fatal } else { [Level::Debug, Level::Info, Level::Warn, Level::Critical][(field & 7).min(3) as usize] };
                match if field == 0xff { 0x1e } else { field >> 3 } {
                    0x1d => self.last_secs += c.varint()? as i64,
                    0x1e => self.last_secs = c.u64()? as i64,
                    d => self.last_secs += d as i64,
                }
                Message { level, category, body: c.string()?, time: self.last_secs * 1000 }
            };
            let filter = self.categories.iter().find(|(n, _)| *n == msg.category).map(|(_, f)| *f).unwrap_or(Filter::from_level(Level::Debug));
            self.recent.push(msg.clone());
            return Some((msg, filter, c.1));
        }
    }
}

// ---- the process logger ----

struct Logger {
    /// Messages before the instance dir exists (upstream keeps them in memfds).
    pending: Vec<(Message, bool)>,
    text: Option<File>,
    detailed: Option<File>,
    encoder: Encoder,
    rules: Vec<Rule>,
    default_level: Level,
    color: bool,
    timestamps: bool,
    /// `--no-detailed-logs`: the detailed log keeps only what is shown.
    sparse: bool,
    filters: HashMap<String, Filter>,
}

fn logger() -> &'static Mutex<Logger> {
    static L: OnceLock<Mutex<Logger>> = OnceLock::new();
    L.get_or_init(|| {
        Mutex::new(Logger {
            pending: Vec::new(),
            text: None,
            detailed: None,
            encoder: Encoder::default(),
            rules: parse_rules(&std::env::var("QT_LOGGING_RULES").unwrap_or_default()),
            default_level: Level::Warn,
            color: true,
            timestamps: false,
            sparse: false,
            filters: HashMap::new(),
        })
    })
}

/// Options from the command line (upstream `--log-rules`, `-v`, `--no-color`, `--log-times`).
pub fn configure(extra_rules: &str, verbosity: u32, color: bool, timestamps: bool, sparse: bool) {
    let mut l = logger().lock().unwrap();
    l.rules.extend(parse_rules(extra_rules));
    l.default_level = match verbosity {
        0 => Level::Warn,
        1 => Level::Info,
        _ => Level::Debug,
    };
    l.color = color;
    l.timestamps = timestamps;
    l.sparse = sparse;
    l.filters.clear();
}

impl Logger {
    fn filter(&mut self, category: &str) -> Filter {
        if let Some(f) = self.filters.get(category) {
            return *f;
        }
        let f = apply_rules(&self.rules, category, base_filter(category, self.default_level));
        self.filters.insert(category.to_string(), f);
        f
    }

    fn record(&mut self, m: Message, shown: bool) {
        if self.detailed.is_none() && self.text.is_none() {
            self.pending.push((m, shown));
            return;
        }
        if shown {
            if let Some(f) = self.text.as_mut() {
                let _ = writeln!(f, "{}", format(&m, false, true, ""));
            }
        }
        if self.detailed.is_some() && (shown || !self.sparse) {
            let mut buf = Vec::new();
            let (rules, level) = (self.rules.clone(), self.default_level);
            self.encoder.encode(&m, |c| apply_rules(&rules, c, base_filter(c, level)), &mut buf);
            if self.detailed.as_mut().unwrap().write_all(&buf).is_err() {
                self.detailed = None;
            }
        }
    }
}

/// A message from Rust code. Shown on stdout when its category's rules allow the level; always
/// kept in the detailed log.
pub fn emit(level: Level, category: &str, body: String) {
    let mut l = logger().lock().unwrap();
    let shown = l.filter(category).shows(level);
    let m = Message { level, category: category.to_string(), body, time: now_ms() };
    crate::errorlog::feed(&m);
    if shown {
        let line = format(&m, l.color, l.timestamps, "");
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
    l.record(m, shown);
}

/// Qt messages arrive already filtered by Qt's own rules, so they are always shown.
fn log_qt_message(kind: i32, category: &str, body: &str) {
    let level = match kind {
        0 => Level::Debug,
        4 => Level::Info,
        1 => Level::Warn,
        2 => Level::Critical,
        _ => Level::Fatal,
    };
    // Upstream turns this category's warnings off (virtual/override property noise).
    if category == "qt.qml.propertyCache.append" && level == Level::Warn && std::env::var_os("QS_NO_FILTER_QT_LOGS").is_none() {
        return;
    }
    let mut l = logger().lock().unwrap();
    let m = Message { level, category: category.to_string(), body: body.to_string(), time: now_ms() };
    crate::errorlog::feed(&m);
    let line = format(&m, l.color, l.timestamps, "");
    {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
    l.record(m, true);
}

/// Opens `dir/log.log` and `dir/log.qslog` (write-locked for `--follow` readers) and writes
/// what was logged so far.
pub fn init_files(dir: &Path) -> std::io::Result<()> {
    let text = File::create(dir.join("log.log"))?;
    let detailed = File::create(dir.join("log.qslog"))?;
    lock(&detailed, libc::F_WRLCK as i16, false);
    let mut l = logger().lock().unwrap();
    l.text = Some(text);
    l.detailed = Some(detailed);
    let _ = l.detailed.as_mut().unwrap().write_all(&Encoder::header());
    for (m, shown) in std::mem::take(&mut l.pending) {
        l.record(m, shown);
    }
    Ok(())
}

/// fcntl record lock over the whole file; `wait` blocks until it can be taken.
fn lock(f: &File, kind: i16, wait: bool) -> bool {
    use std::os::fd::AsRawFd;
    let mut fl: libc::flock = unsafe { std::mem::zeroed() };
    fl.l_type = kind;
    fl.l_whence = libc::SEEK_SET as i16;
    let cmd = if wait { libc::F_SETLKW } else { libc::F_SETLK };
    unsafe { libc::fcntl(f.as_raw_fd(), cmd, &fl) == 0 }
}

/// `log` subcommand: prints `path` like upstream's readEncodedLogs. `tail` 0 prints everything.
pub fn read_file(path: &PathBuf, tail: usize, follow: bool, rules: &str, color: bool, timestamps: bool) -> i32 {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open log file {}: {e}", path.display());
            return 255;
        }
    };
    let rules = parse_rules(rules);
    let mut data = Vec::new();
    let _ = file.read_to_end(&mut data);
    let mut pos = match Decoder::header(&data) {
        Ok(p) => p,
        Err(v) => {
            eprintln!("This log was encoded with version {v} of the quickshell log encoder, which cannot be decoded by the current version of quickshell ({LOG_VERSION}).");
            return 255;
        }
    };
    let mut decoder = Decoder::default();
    let mut filters: HashMap<String, Filter> = HashMap::new();
    let mut shown = |m: &Message, file_filter: Filter| {
        let f = *filters.entry(m.category.clone()).or_insert_with(|| apply_rules(&rules, &m.category, file_filter));
        f.shows(m.level)
    };
    let mut out = std::io::stdout().lock();
    let mut buffered: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    while let Some((m, f, next)) = decoder.next(&data, pos) {
        pos = next;
        if shown(&m, f) {
            let line = format(&m, color, timestamps, "");
            if tail == 0 {
                let _ = writeln!(out, "{line}");
            } else {
                if buffered.len() == tail {
                    buffered.pop_front();
                }
                buffered.push_back(line);
            }
        }
    }
    for line in buffered {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
    if !follow {
        return 0;
    }
    // The writer holds a write lock for its whole life: follow until it lets go.
    loop {
        let alive = !lock(&file, libc::F_RDLCK as i16, false);
        let mut more = Vec::new();
        let _ = file.read_to_end(&mut more);
        data.extend_from_slice(&more);
        while let Some((m, f, next)) = decoder.next(&data, pos) {
            pos = next;
            if shown(&m, f) {
                let _ = writeln!(out, "{}", format(&m, color, timestamps, ""));
            }
        }
        let _ = out.flush();
        if !alive {
            return 0;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Every message of a log file, in order, to `each`; with `follow`, keeps reading until the
/// writing process lets go of the file. `Err` is the text to print when the file can't be read.
pub fn each_message(path: &Path, follow: bool, mut each: impl FnMut(&Message)) -> Result<(), String> {
    let mut file = File::open(path).map_err(|e| format!("Failed to open log file {}: {e}", path.display()))?;
    let mut data = Vec::new();
    let _ = file.read_to_end(&mut data);
    let mut pos = Decoder::header(&data).map_err(|v| format!("This log was encoded with version {v} of the log encoder, which this build cannot decode ({LOG_VERSION})."))?;
    let mut decoder = Decoder::default();
    loop {
        let alive = follow && !lock(&file, libc::F_RDLCK as i16, false);
        while let Some((m, _, next)) = decoder.next(&data, pos) {
            pos = next;
            each(&m);
        }
        if !alive {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = file.read_to_end(&mut data);
    }
}

/// Logs through the instance logger: `qs_log!(warn, "quickshell.service.mpris", "...", args)`.
#[macro_export]
macro_rules! qs_log {
    (debug, $cat:expr, $($arg:tt)*) => { $crate::log::emit($crate::log::Level::Debug, $cat, format!($($arg)*)) };
    (info, $cat:expr, $($arg:tt)*) => { $crate::log::emit($crate::log::Level::Info, $cat, format!($($arg)*)) };
    (warn, $cat:expr, $($arg:tt)*) => { $crate::log::emit($crate::log::Level::Warn, $cat, format!($($arg)*)) };
    (error, $cat:expr, $($arg:tt)*) => { $crate::log::emit($crate::log::Level::Critical, $cat, format!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(level: Level, cat: &str, body: &str, time: i64) -> Message {
        Message { level, category: cat.into(), body: body.into(), time }
    }

    #[test]
    fn format_like_upstream() {
        let m = msg(Level::Warn, "qml", "x", 0);
        assert_eq!(format(&m, false, false, ""), "  WARN qml: x");
        assert_eq!(format(&msg(Level::Info, "default", "Configuration Loaded", 0), false, false, ""), "  INFO: Configuration Loaded");
        assert_eq!(format(&msg(Level::Critical, "a", "b", 0), true, false, ""), "\x1b[31m ERROR\x1b[97m a\x1b[0m: b");
    }

    #[test]
    fn rules_match_like_qt() {
        let rules = parse_rules("quickshell.service.*=true;*.debug=false;qml.warning=false");
        let base = Filter::from_level(Level::Warn);
        let f = apply_rules(&rules, "quickshell.service.mpris", base);
        assert!(f.info && !f.debug && f.warn);
        assert!(!apply_rules(&rules, "qml", base).warn);
        assert!(!apply_rules(&rules, "quickshell.dbus", base).info);
    }

    #[test]
    fn qslog_round_trip() {
        let t0 = 1_760_000_000_000;
        let msgs = vec![
            msg(Level::Info, "default", "Configuration Loaded", t0),
            msg(Level::Warn, "qml", "a warning", t0 + 300),
            msg(Level::Warn, "qml", "a warning", t0 + 2_000),
            msg(Level::Debug, "quickshell.service.mpris", "x".repeat(300).as_str(), t0 + 40_000),
            msg(Level::Info, "default", "Configuration Loaded", t0 + 200_000),
            msg(Level::Critical, "qml", "late", t0 + 100_000_000),
        ];
        let mut enc = Encoder::default();
        let mut data = Encoder::header().to_vec();
        for m in &msgs {
            enc.encode(m, |_| Filter::from_level(Level::Warn), &mut data);
        }
        let mut dec = Decoder::default();
        let mut pos = Decoder::header(&data).unwrap();
        let mut out = Vec::new();
        while let Some((m, f, next)) = dec.next(&data, pos) {
            assert_eq!(f, Filter::from_level(Level::Warn));
            out.push(m);
            pos = next;
        }
        assert_eq!(pos, data.len());
        let secs = |v: &[Message]| v.iter().map(|m| (m.level, m.category.clone(), m.body.clone(), m.time / 1000)).collect::<Vec<_>>();
        assert_eq!(secs(&out), secs(&msgs));
        // A half-written record is not read.
        let (mut dec, mut pos, mut n) = (Decoder::default(), 1, 0);
        while let Some((_, _, next)) = dec.next(&data[..data.len() - 2], pos) {
            pos = next;
            n += 1;
        }
        assert_eq!(n, msgs.len() - 1);
    }
}
