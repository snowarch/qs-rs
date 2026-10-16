//! Child process plumbing shared by `Process` and `Quickshell.execDetached`. Pure Rust: spawning,
//! pipe reader threads, environment rules, and conversion of exec() arguments.

use cxx_qt_lib::{QHash, QHashPair_QString_QVariant, QMap, QMapPair_QString_QVariant, QString, QStringList, QVariant};

type QVariantMap = QMap<QMapPair_QString_QVariant>;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Sender, channel};
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
    pub stdin: Option<Sender<Vec<u8>>>,
}

fn command(ctx: &Context) -> Command {
    let mut cmd = Command::new(&ctx.command[0]);
    cmd.args(&ctx.command[1..]);
    if !ctx.working_directory.is_empty() {
        cmd.current_dir(&ctx.working_directory);
    }
    let initial = quickshell::common::initial_env();
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
    let emit = std::sync::Arc::new(emit);

    let reader = |mut pipe: Box<dyn Read + Send>, wrap: fn(Vec<u8>) -> Event, emit: std::sync::Arc<dyn Fn(Event) + Send + Sync>| {
        thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => emit(wrap(buf[..n].to_vec())),
                }
            }
        })
    };
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(reader(Box::new(out), Event::Stdout, emit.clone()));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(reader(Box::new(err), Event::Stderr, emit.clone()));
    }
    let stdin = child.stdin.take().map(|mut pipe| {
        let (tx, rx) = channel::<Vec<u8>>();
        thread::spawn(move || {
            for data in rx {
                if pipe.write_all(&data).and_then(|_| pipe.flush()).is_err() {
                    break;
                }
            }
        });
        tx
    });
    thread::spawn(move || {
        let status = child.wait();
        for r in readers {
            let _ = r.join();
        }
        use std::os::unix::process::ExitStatusExt;
        let (code, crashed) = match status {
            Ok(s) => match s.code() {
                Some(c) => (c, false),
                None => (s.signal().unwrap_or(0), true),
            },
            Err(_) => (-1, true),
        };
        emit(Event::Exited { code, crashed });
    });
    Ok(Spawned { pid, stdin })
}

pub fn spawn_detached(ctx: &Context) {
    if ctx.command.is_empty() {
        return;
    }
    let mut cmd = command(ctx);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
    match cmd.spawn() {
        // Reap in the background so the child never lingers as a zombie.
        Ok(mut child) => drop(thread::spawn(move || child.wait())),
        Err(e) => eprintln!("Failed to start detached process {:?}: {e}", ctx.command),
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
