//! A cold start compiles every QML file on Qt's one loader thread: 1.5 s of inir's 2.4 s start, and every update
//! leaves the files it touched cold. When many of the files the shell will load are not in Qt's disk cache, qs-rs
//! compiles them first in a few `check` workers at once, into the same cache through the same VFS paths, and the
//! shell then loads warm. A worker is its own process (Qt's loader is not shared across threads): a crash or a bad
//! file costs only its own share, and the shell compiles what is missing as before.
//!
//! What gets compiled: after an update, the files the cache holds compiled from other contents (what the shell
//! loaded before and changed); on a first start, with nothing compiled yet, every QML file of the shell.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Below this many files, starting workers costs more than the loader thread compiling them.
const MIN_FILES: usize = 120;
const FILES_PER_WORKER: usize = 60;
const MAX_WORKERS: usize = 8;
/// A worker's peak: Qt, the engine and what it compiled.
const WORKER_MB: u64 = 200;
/// Past this the shell stops waiting and loads; what a worker left half done Qt compiles again.
const TIMEOUT: Duration = Duration::from_secs(20);

pub struct Workers {
    children: Vec<Child>,
    files: usize,
    started: Instant,
}

/// Whether Qt's disk cache holds `source` compiled: 0 no entry, 1 an entry compiled from other contents, 2 current.
/// Qt 6.12 names the entry sha1(local path) + "." + the suffix after the name's first dot + "c", and takes it while the
/// md5 of the source equals the 16 bytes at offset 28 of its header (the time stamp there is -1).
fn cache_state(cache: &Path, source: &Path) -> u8 {
    let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let suffix = name.split_once('.').map_or(String::new(), |(_, s)| format!("{s}c"));
    let entry = cache.join(format!("{}.{suffix}", crate::host::ffi::sha1_hex(&source.to_string_lossy())));
    let mut head = [0u8; 44];
    let Ok(mut file) = std::fs::File::open(&entry) else { return 0 };
    if file.read_exact(&mut head).is_err() || &head[..8] != b"qv4cdata" {
        return 0;
    }
    let Ok(text) = std::fs::read(source) else { return 0 };
    if crate::host::ffi::md5_of(&text).as_slice() == &head[28..44] { 2 } else { 1 }
}

/// The QML files (relative to the shell) to compile before the shell loads, and what the cache held.
fn plan(qml: Vec<PathBuf>, vfs: &Path, cache: &Path) -> (Vec<PathBuf>, String) {
    let (mut missing, mut stale, mut current) = (Vec::new(), Vec::new(), 0);
    for rel in qml {
        match cache_state(cache, &vfs.join(&rel)) {
            0 => missing.push(rel),
            1 => stale.push(rel),
            _ => current += 1,
        }
    }
    let summary = format!("QML cache: {current} current, {} changed, {} not compiled", stale.len(), missing.len());
    (if current == 0 && stale.is_empty() { missing } else { stale }, summary)
}

fn available_mb() -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = meminfo.lines().find(|l| l.starts_with("MemAvailable:"))?;
    Some(line.split_whitespace().nth(1)?.parse::<u64>().ok()? / 1024)
}

/// Starts the workers on a thread of their own, while the shell sets up Qt; `Pending::wait` before the root loads.
pub fn start(scan: &crate::scan::Scan, vfs_root: &Path, config: &Path) -> Option<Pending> {
    if std::env::var_os("QML_DISABLE_DISK_CACHE").is_some() || std::env::var_os("QSRS_NO_PRECOMPILE").is_some() {
        return None;
    }
    let cache = crate::cachesync::qt_cache_dir()?;
    let min = std::env::var("QSRS_PRECOMPILE_MIN").ok().and_then(|v| v.parse().ok()).unwrap_or(MIN_FILES);
    let qml: Vec<PathBuf> = scan
        .files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "qml"))
        .filter_map(|f| f.strip_prefix(&scan.root).ok().map(Path::to_path_buf))
        .collect();
    if qml.len() < min {
        return None;
    }
    let (vfs_root, config) = (vfs_root.to_path_buf(), config.to_path_buf());
    let thread = std::thread::Builder::new()
        .name("qs-precompile".into())
        .spawn(move || {
            let (files, summary) = plan(qml, &vfs_root.join("qs"), &cache);
            let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
            let memory = available_mb().map_or(MAX_WORKERS, |mb| (mb / WORKER_MB / 2) as usize);
            let n = cores.saturating_sub(1).min(MAX_WORKERS).min(memory).min(files.len().div_ceil(FILES_PER_WORKER));
            let summary = format!("{summary}; {} to compile ahead, {n} workers ({cores} cores)", files.len());
            let workers = if files.len() < min || n < 2 { None } else { spawn(&files, n, &vfs_root, &config) };
            (workers, summary)
        })
        .ok()?;
    Some(Pending(thread))
}

pub struct Pending(std::thread::JoinHandle<(Option<Workers>, String)>);

impl Pending {
    pub fn wait(self) {
        match self.0.join() {
            Ok((workers, summary)) => {
                quickshell::qs_log!(debug, "quickshell.qsrs", "{summary}");
                if let Some(workers) = workers {
                    workers.wait();
                }
            }
            Err(_) => quickshell::qs_log!(warn, "quickshell.qsrs", "The QML precompile planner panicked; loading as usual"),
        }
    }
}

fn spawn(files: &[PathBuf], n: usize, vfs_root: &Path, config: &Path) -> Option<Workers> {
    let exe = std::env::current_exe().ok()?;
    crate::cachesync::hold();
    let mut children = Vec::new();
    for _ in 0..n {
        let child = Command::new(&exe)
            .args(["check", "-p"])
            .arg(config)
            .env("QSRS_PRECOMPILE_VFS", vfs_root)
            .env("QT_QPA_PLATFORM", "offscreen")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(child) = child {
            children.push(child);
        }
    }
    // Shares go out once every worker is starting: a share past the pipe's size waits for its reader.
    // Interleaved, so every worker gets files from every part of the tree.
    let n = children.len();
    for (k, child) in children.iter_mut().enumerate() {
        let share: String = files.iter().skip(k).step_by(n.max(1)).map(|f| format!("{}\n", f.display())).collect();
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(share.as_bytes());
        }
    }
    if children.is_empty() {
        crate::cachesync::release();
        return None;
    }
    Some(Workers { children, files: files.len(), started: Instant::now() })
}

impl Workers {
    fn wait(mut self) {
        let deadline = self.started + TIMEOUT;
        let mut failed = 0;
        for child in &mut self.children {
            loop {
                match child.try_wait() {
                    Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(4)),
                    Ok(None) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        failed += 1;
                        break;
                    }
                    Ok(Some(status)) => {
                        failed += usize::from(!status.success());
                        break;
                    }
                    Err(_) => break,
                }
            }
        }
        crate::cachesync::release();
        quickshell::qs_log!(
            info,
            "quickshell.qsrs",
            "Compiled {} QML files ahead in {} workers, {} ms{}",
            self.files,
            self.children.len(),
            self.started.elapsed().as_millis(),
            if failed > 0 { format!(" ({failed} did not finish; the shell compiles what they left)") } else { String::new() }
        );
    }
}

/// The worker's side, inside `check`: compiles each file named on stdin through the shell's VFS, writing Qt's
/// cache, and reports nothing (the shell reports errors when it loads them).
pub fn work(vfs: &Path, mut engine: std::pin::Pin<&mut cxx_qt_lib::QQmlEngine>) -> i32 {
    let mut list = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut list);
    for rel in list.lines().filter(|l| !l.is_empty()) {
        let _ = crate::host::ffi::check_file(engine.as_mut(), &crate::qstr(&vfs.join(rel)));
    }
    0
}
