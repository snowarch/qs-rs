//! Child process plumbing shared by `Process` and `Quickshell.execDetached`. Pure Rust: spawning,
//! one I/O thread for every child, environment rules, and conversion of exec() arguments.

use cxx_qt_lib::{QHash, QHashPair_QString_QVariant, QMap, QMapPair_QString_QVariant, QString, QStringList, QVariant};

type QVariantMap = QMap<QMapPair_QString_QVariant>;
use std::collections::HashMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;

pub enum Event {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Exited { code: i32, crashed: bool },
}

#[derive(Default, Debug)]
pub struct Context {
    pub command: Vec<String>,
    pub working_directory: String,
    /// `None` unsets the variable (or restores it from the initial env when clearing).
    pub environment: Vec<(String, Option<String>)>,
    pub clear_environment: bool,
}

pub struct Spawned {
    pub pid: u32,
    pub stdin: Option<Stdin>,
}

/// Write end of a child's stdin; dropping it closes the pipe.
pub struct Stdin(u64);

impl Stdin {
    pub fn send(&self, data: Vec<u8>) -> Result<(), ()> {
        reactor().command(Cmd::Write(self.0, data));
        Ok(())
    }
}

impl Drop for Stdin {
    fn drop(&mut self) {
        reactor().command(Cmd::CloseStdin(self.0));
    }
}

fn command(ctx: &Context) -> Command {
    let mut cmd = Command::new(&ctx.command[0]);
    cmd.args(&ctx.command[1..]);
    if !ctx.working_directory.is_empty() {
        cmd.current_dir(&ctx.working_directory);
    }
    let initial = crate::common::initial_env();
    cmd.env_clear();
    let mut env: Vec<(String, String)> = if ctx.clear_environment { Vec::new() } else { initial.to_vec() };
    for (name, value) in &ctx.environment {
        env.retain(|(k, _)| k != name);
        match value {
            Some(v) => env.push((name.clone(), v.clone())),
            None if ctx.clear_environment => {
                if let Some(kv) = initial.iter().find(|(k, _)| k == name) {
                    env.push(kv.clone());
                }
            }
            None => {}
        }
    }
    cmd.envs(env);
    cmd
}

pub fn spawn(
    ctx: &Context,
    want_out: bool,
    want_err: bool,
    want_in: bool,
    emit: impl Fn(Event) + Send + Sync + 'static,
) -> std::io::Result<Spawned> {
    let mut cmd = command(ctx);
    cmd.stdin(if want_in { Stdio::piped() } else { Stdio::null() });
    cmd.stdout(if want_out { Stdio::piped() } else { Stdio::null() });
    cmd.stderr(if want_err { Stdio::piped() } else { Stdio::null() });
    let mut child = cmd.spawn()?;
    let pid = child.id();
    let out = child.stdout.take().map(OwnedFd::from);
    let err = child.stderr.take().map(OwnedFd::from);
    let inp = child.stdin.take().map(OwnedFd::from);
    let has_in = inp.is_some();
    let id = reactor().add(child, out, err, inp, Some(Box::new(emit)))?;
    Ok(Spawned { pid, stdin: has_in.then_some(Stdin(id)) })
}

pub fn spawn_detached(ctx: &Context) {
    if ctx.command.is_empty() {
        return;
    }
    let mut cmd = command(ctx);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
    // Reaped by the reactor so the child never lingers as a zombie.
    let started = cmd.spawn().and_then(|child| reactor().add(child, None, None, None, None));
    if let Err(e) = started {
        crate::qs_log!(warn, "quickshell.io.process", "Failed to start detached process {:?}: {e}", ctx.command);
    }
}

// One thread ("qs-proc") serves every child: pipes and exit (pidfd) multiplexed with poll(2),
// like the event loop QProcess runs on upstream. Per child, output events come in order and
// `Exited` comes last, after what the pipes held at exit was read; a grandchild that keeps a
// pipe open does not hold the exit back (QProcess behaves the same).

type Emit = Box<dyn Fn(Event) + Send + Sync>;

enum Cmd {
    Add(u64, Box<Child_>),
    Write(u64, Vec<u8>),
    CloseStdin(u64),
}

struct Child_ {
    child: Child,
    pidfd: OwnedFd,
    out: Option<OwnedFd>,
    err: Option<OwnedFd>,
    inp: Option<OwnedFd>,
    pending: Vec<u8>,
    close_in: bool,
    emit: Option<Emit>,
}

struct Reactor {
    cmds: Mutex<Vec<Cmd>>,
    wake: OwnedFd,
    next: AtomicU64,
}

fn reactor() -> &'static Reactor {
    static R: OnceLock<Reactor> = OnceLock::new();
    R.get_or_init(|| {
        let wake = unsafe { OwnedFd::from_raw_fd(libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK)) };
        thread::Builder::new().name("qs-proc".into()).spawn(run).expect("process reactor thread");
        Reactor { cmds: Mutex::new(Vec::new()), wake, next: AtomicU64::new(1) }
    })
}

impl Reactor {
    fn command(&self, cmd: Cmd) {
        self.cmds.lock().unwrap().push(cmd);
        let one: u64 = 1;
        unsafe { libc::write(self.wake.as_raw_fd(), (&raw const one).cast(), 8) };
    }

    fn add(&self, child: Child, out: Option<OwnedFd>, err: Option<OwnedFd>, inp: Option<OwnedFd>, emit: Option<Emit>) -> std::io::Result<u64> {
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id() as libc::pid_t, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let pidfd = unsafe { OwnedFd::from_raw_fd(fd as RawFd) };
        for f in [&out, &err, &inp].into_iter().flatten() {
            unsafe { libc::fcntl(f.as_raw_fd(), libc::F_SETFL, libc::fcntl(f.as_raw_fd(), libc::F_GETFL) | libc::O_NONBLOCK) };
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let c = Child_ { child, pidfd, out, err, inp, pending: Vec::new(), close_in: false, emit };
        self.command(Cmd::Add(id, Box::new(c)));
        Ok(id)
    }
}

/// Reads what `fd` has now. Returns false once it reached EOF or failed.
fn drain(fd: &OwnedFd, wrap: fn(Vec<u8>) -> Event, emit: &Option<Emit>, once: bool) -> bool {
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n > 0 {
            if let Some(e) = emit {
                e(wrap(buf[..n as usize].to_vec()));
            }
            if once {
                return true;
            }
        } else if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
            return true;
        } else if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        } else {
            return false;
        }
    }
}

fn run() {
    let r = reactor();
    let mut procs: HashMap<u64, Child_> = HashMap::new();
    let mut fds: Vec<libc::pollfd> = Vec::new();
    let mut slots: Vec<(u64, u8)> = Vec::new(); // (child id, 0 pidfd / 1 out / 2 err / 3 in)
    loop {
        for cmd in std::mem::take(&mut *r.cmds.lock().unwrap()) {
            match cmd {
                Cmd::Add(id, c) => drop(procs.insert(id, *c)),
                Cmd::Write(id, data) => {
                    if let Some(c) = procs.get_mut(&id) {
                        c.pending.extend(data);
                    }
                }
                Cmd::CloseStdin(id) => {
                    if let Some(c) = procs.get_mut(&id) {
                        c.close_in = true;
                    }
                }
            }
        }
        fds.clear();
        slots.clear();
        fds.push(libc::pollfd { fd: r.wake.as_raw_fd(), events: libc::POLLIN, revents: 0 });
        slots.push((0, 255));
        for (&id, c) in procs.iter_mut() {
            if c.close_in && c.pending.is_empty() {
                c.inp = None;
            }
            fds.push(libc::pollfd { fd: c.pidfd.as_raw_fd(), events: libc::POLLIN, revents: 0 });
            slots.push((id, 0));
            for (slot, fd, events) in [(1, &c.out, libc::POLLIN), (2, &c.err, libc::POLLIN)] {
                if let Some(fd) = fd {
                    fds.push(libc::pollfd { fd: fd.as_raw_fd(), events, revents: 0 });
                    slots.push((id, slot));
                }
            }
            if let Some(fd) = c.inp.as_ref().filter(|_| !c.pending.is_empty()) {
                fds.push(libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLOUT, revents: 0 });
                slots.push((id, 3));
            }
        }
        if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) } < 0 {
            continue;
        }
        let mut exited = Vec::new();
        for (pfd, &(id, slot)) in fds.iter().zip(&slots) {
            if pfd.revents == 0 {
                continue;
            }
            if slot == 255 {
                let mut v = 0u64;
                unsafe { libc::read(r.wake.as_raw_fd(), (&raw mut v).cast(), 8) };
                continue;
            }
            let Some(c) = procs.get_mut(&id) else { continue };
            match slot {
                0 => exited.push(id),
                1 if !c.out.as_ref().is_some_and(|f| drain(f, Event::Stdout, &c.emit, true)) => c.out = None,
                2 if !c.err.as_ref().is_some_and(|f| drain(f, Event::Stderr, &c.emit, true)) => c.err = None,
                3 => {
                    let fd = c.inp.as_ref().unwrap().as_raw_fd();
                    let n = unsafe { libc::write(fd, c.pending.as_ptr().cast(), c.pending.len()) };
                    if n > 0 {
                        c.pending.drain(..n as usize);
                    } else if std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock {
                        c.pending.clear();
                        c.inp = None;
                    }
                }
                _ => {}
            }
        }
        for id in exited {
            let Some(mut c) = procs.remove(&id) else { continue };
            for (fd, wrap) in [(&c.out, Event::Stdout as fn(Vec<u8>) -> Event), (&c.err, Event::Stderr)] {
                if let Some(fd) = fd {
                    drain(fd, wrap, &c.emit, false);
                }
            }
            use std::os::unix::process::ExitStatusExt;
            let (code, crashed) = match c.child.try_wait() {
                Ok(Some(s)) => match s.code() {
                    Some(code) => (code, false),
                    None => (s.signal().unwrap_or(0), true),
                },
                _ => (-1, true),
            };
            if let Some(e) = &c.emit {
                e(Event::Exited { code, crashed });
            }
        }
    }
}

pub fn kill(pid: u32, signal: i32) {
    unsafe {
        libc::kill(pid as i32, signal);
    }
}

fn variant_to_env(v: &QVariant) -> Option<Option<String>> {
    match v.type_name() {
        "" => None,
        "std::nullptr_t" => Some(None),
        _ => Some(Some(v.value::<QString>().map(|s| s.to_string()).unwrap_or_default())),
    }
}

pub fn env_from_hash(hash: &QHash<QHashPair_QString_QVariant>) -> Vec<(String, Option<String>)> {
    hash.iter().filter_map(|(k, v)| variant_to_env(v).map(|v| (k.to_string(), v))).collect()
}

#[derive(Default)]
pub struct ExecArgs {
    pub command: Option<Vec<String>>,
    pub environment: Option<QHash<QHashPair_QString_QVariant>>,
    pub clear_environment: Option<bool>,
    pub working_directory: Option<String>,
}

fn string_list(v: &QVariant) -> Option<Vec<String>> {
    v.value::<QStringList>().map(|l| l.iter().map(|s| s.to_string()).collect())
}

/// Accepts either a command list or `{ command, environment, clearEnvironment, workingDirectory }`.
pub fn context_from_variant(v: &QVariant) -> ExecArgs {
    if let Some(map) = v.value::<QVariantMap>().filter(|m| m.len() > 0) {
        let mut out = ExecArgs { command: map.get(&QString::from("command")).and_then(|c| string_list(&c)), ..Default::default() };
        if let Some(env) = map.get(&QString::from("environment")) {
            if let Some(m) = env.value::<QVariantMap>() {
                let mut hash = QHash::<QHashPair_QString_QVariant>::default();
                for (k, v) in m.iter() {
                    hash.insert_clone(k, v);
                }
                out.environment = Some(hash);
            }
        }
        out.clear_environment = map.get(&QString::from("clearEnvironment")).and_then(|b| b.value::<bool>());
        out.working_directory =
            map.get(&QString::from("workingDirectory")).and_then(|d| d.value::<QString>()).map(|d| d.to_string());
        return out;
    }
    ExecArgs { command: string_list(v), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::Duration;

    fn run(cmd: &[&str], want_in: bool) -> (Spawned, Receiver<Event>) {
        let ctx = Context { command: cmd.iter().map(|s| s.to_string()).collect(), ..Default::default() };
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let spawned = spawn(&ctx, true, true, want_in, move |e| drop(tx.lock().unwrap().send(e))).unwrap();
        (spawned, rx)
    }

    /// Collects stdout until Exited; panics if it takes longer than `secs`.
    fn finish(rx: &Receiver<Event>, secs: u64) -> (String, i32) {
        let mut out = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(secs)).expect("no exit in time") {
                Event::Stdout(d) => out.extend(d),
                Event::Stderr(_) => {}
                Event::Exited { code, .. } => return (String::from_utf8(out).unwrap(), code),
            }
        }
    }

    fn threads_named(name: &str) -> usize {
        std::fs::read_dir("/proc/self/task")
            .unwrap()
            .flatten()
            .filter(|t| std::fs::read_to_string(t.path().join("comm")).is_ok_and(|c| c.trim() == name))
            .count()
    }

    #[test]
    fn output_then_exit() {
        let (_s, rx) = run(&["sh", "-c", "echo one; echo two >&2; echo three; exit 4"], false);
        assert_eq!(finish(&rx, 5), ("one\nthree\n".into(), 4));
    }

    #[test]
    fn grandchild_holding_the_pipe_does_not_delay_exit() {
        let (_s, rx) = run(&["sh", "-c", "sleep 5 & echo hi; exit 3"], false);
        assert_eq!(finish(&rx, 2), ("hi\n".into(), 3));
    }

    #[test]
    fn stdin_round_trip_and_close() {
        let (s, rx) = run(&["cat"], true);
        let stdin = s.stdin.unwrap();
        stdin.send(b"abc".to_vec()).unwrap();
        drop(stdin);
        assert_eq!(finish(&rx, 5), ("abc".into(), 0));
    }

    #[test]
    fn one_thread_for_many_children() {
        let runs: Vec<_> = (0..20).map(|_| run(&["sh", "-c", "sleep 0.3; echo ok"], true)).collect();
        assert_eq!(threads_named("qs-proc"), 1);
        assert!(std::fs::read_dir("/proc/self/task").unwrap().count() < 15);
        for (_s, rx) in &runs {
            assert_eq!(finish(rx, 5), ("ok\n".into(), 0));
        }
    }
}
