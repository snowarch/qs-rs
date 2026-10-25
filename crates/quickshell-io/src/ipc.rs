//! `IpcHandler` and the IPC server, speaking upstream Quickshell's protocol (ipc/ipc.cpp,
//! io/ipccomm.cpp) on `<instance dir>/ipc.sock`, so `qs -p <shell> ipc call ...` reaches qs-rs.
//! Handlers register by target; each connection runs on its own thread, which forwards requests
//! to the handler's Qt thread and waits for the reply.
//!
//! Wire format: QDataStream values back to back. A command is a variant (u8 index: 1 kill,
//! 2 query metadata, 3 call, 4 listen to signal, 5 read property; qs-rs only: 6 frame meter)
//! followed by its fields; each reply is a variant of the command's response type.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("quickshell_io/include/ipcmeta.h");
        #[namespace = "qsrs_io"]
        fn ipc_call(handler: Pin<&mut IpcHandler>, name: &QString, args: &QStringList, out: &mut QString) -> i32;
        #[namespace = "qsrs_io"]
        fn ipc_describe(handler: &IpcHandler) -> QStringList;
        #[namespace = "qsrs_io"]
        fn ipc_read_property(handler: &IpcHandler, name: &QString, out: &mut QString) -> bool;
        #[namespace = "qsrs_io"]
        fn ipc_quit();
        #[namespace = "qsrs_io"]
        fn ipc_connect_signals(handler: Pin<&mut IpcHandler>);
        #[namespace = "qsrs_io"]
        fn ipc_value_string(value: &QVariant) -> QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, enabled, READ, WRITE = set_enabled, NOTIFY)]
        #[qproperty(QString, target, READ, WRITE = set_target, NOTIFY)]
        type IpcHandler = super::IpcHandlerRust;

        fn set_enabled(self: Pin<&mut IpcHandler>, enabled: bool);
        fn set_target(self: Pin<&mut IpcHandler>, target: QString);
        /// Called from the JS closures ipc_connect_signals attaches to each signal.
        #[qinvokable]
        #[cxx_name = "_qsSignal"]
        fn signal_fired(self: &IpcHandler, name: &QString, value: &QVariant, is_void: bool);
    }

    impl cxx_qt::Threading for IpcHandler {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::{QString, QStringList, QVariant};
use quickshell::datastream::{Incomplete, Reader, Writer};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Mutex, mpsc};

type Thread = CxxQtThread<qobject::IpcHandler>;

/// target -> every handler asking for it as (registration id, generation, thread); the first one answers.
/// Upstream keeps a registry per engine generation and promotes the next known handler when one goes. With one
/// registry here, a reload's new handler was refused while the old one lived and the target vanished with it.
static REGISTRY: Mutex<Option<HashMap<String, Vec<(u64, u64, Thread)>>>> = Mutex::new(None);
static NEXT_ID: Mutex<u64> = Mutex::new(0);

pub struct IpcHandlerRust {
    enabled: bool,
    target: QString,
    id: u64,
    registered: Option<String>,
    signals_connected: bool,
}

impl Default for IpcHandlerRust {
    fn default() -> Self {
        let mut n = NEXT_ID.lock().unwrap();
        *n += 1;
        Self { enabled: true, target: QString::default(), id: *n, registered: None, signals_connected: false }
    }
}

impl Drop for IpcHandlerRust {
    fn drop(&mut self) {
        if let Some(t) = self.registered.take() {
            unregister(&t, self.id);
        }
    }
}

fn unregister(target: &str, id: u64) {
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(map) = reg.as_mut()
        && let Some(known) = map.get_mut(target) {
            known.retain(|(rid, _, _)| *rid != id);
            if known.is_empty() {
                map.remove(target);
            }
        }
}

impl qobject::IpcHandler {
    fn set_enabled(mut self: Pin<&mut Self>, enabled: bool) {
        if enabled != self.enabled {
            self.as_mut().rust_mut().enabled = enabled;
            self.as_mut().enabled_changed();
            self.update_registration();
        }
    }

    fn set_target(mut self: Pin<&mut Self>, target: QString) {
        if target != self.target {
            self.as_mut().rust_mut().target = target;
            self.as_mut().target_changed();
            self.update_registration();
        }
    }

    fn update_registration(mut self: Pin<&mut Self>) {
        let id = self.id;
        if let Some(old) = self.as_mut().rust_mut().registered.take() {
            unregister(&old, id);
        }
        let target = self.target.to_string();
        if !self.enabled || target.is_empty() {
            return;
        }
        {
            let generation = quickshell::GENERATION.load(std::sync::atomic::Ordering::SeqCst);
            let mut reg = REGISTRY.lock().unwrap();
            let known = reg.get_or_insert_with(HashMap::new).entry(target.clone()).or_default();
            match known.first() {
                // a reload's handler takes over from the generation going away
                Some((_, older, _)) if *older < generation => known.insert(0, (id, generation, self.qt_thread())),
                Some(_) => {
                    quickshell::qs_log!(warn, "quickshell.ipc", "target {target:?} is already registered, ignoring the duplicate.");
                    known.push((id, generation, self.qt_thread()));
                }
                None => known.push((id, generation, self.qt_thread())),
            }
        }
        self.as_mut().rust_mut().registered = Some(target);
    }
}

/// Remote `ipc listen`/`ipc wait` clients: (connection, target, signal, writer queue).
static LISTENERS: Mutex<Vec<(u64, String, String, mpsc::Sender<String>)>> = Mutex::new(Vec::new());

impl qobject::IpcHandler {
    fn signal_fired(&self, name: &QString, value: &QVariant, is_void: bool) {
        let Some(target) = self.registered.as_deref() else { return };
        // only the handler answering for the target speaks for it
        if handler_id(target) != Some(self.id) {
            return;
        }
        let value = if is_void { "void".to_string() } else { qobject::ipc_value_string(value).to_string() };
        let name = name.to_string();
        LISTENERS.lock().unwrap().retain(|(_, t, s, tx)| t != target || *s != name || tx.send(value.clone()).is_ok());
    }
}

/// Upstream SignalListenCommand::exec: checks the target and signal, then streams one
/// SignalResponse (variant 4) per emission until the client hangs up.
fn listen(conn: u64, stream: &UnixStream, target: String, signal: String) -> Option<Vec<u8>> {
    let mut w = Writer::default();
    let Some(def) = describe(&target) else { return Some(w.u8(TARGET_NOT_FOUND).0.clone()) };
    if !def.signals.iter().any(|(n, _, _)| *n == signal) {
        return Some(w.u8(ENTRY_NOT_FOUND).0.clone());
    }
    on_handler(&target, |mut h| {
        if !h.signals_connected {
            h.as_mut().rust_mut().signals_connected = true;
            qobject::ipc_connect_signals(h);
        }
    })?;
    let (tx, rx) = mpsc::channel::<String>();
    let mut out = stream.try_clone().ok()?;
    LISTENERS.lock().unwrap().push((conn, target, signal, tx));
    std::thread::spawn(move || {
        for value in rx {
            let mut w = Writer::default();
            w.u8(4).string(&value);
            if out.write_all(&w.0).is_err() {
                return;
            }
        }
    });
    None
}

/// Listens on `<instance dir>/ipc.sock` (upstream IpcServer::start, run at launch).
pub fn start_server(instance_dir: &Path) {
    let path = instance_dir.join("ipc.sock");
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            quickshell::qs_log!(warn, "quickshell.ipc", "could not start the server on {}: {e}", path.display());
            return;
        }
    };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve(stream));
        }
    });
}

fn handler_id(target: &str) -> Option<u64> {
    REGISTRY.lock().unwrap().as_ref().and_then(|m| m.get(target).and_then(|known| known.first()).map(|(id, _, _)| *id))
}

fn handler_thread(target: &str) -> Option<Thread> {
    REGISTRY.lock().unwrap().as_ref().and_then(|m| m.get(target).and_then(|known| known.first()).map(|(_, _, t)| t.clone()))
}

/// Runs `f` on the handler's Qt thread and waits for its result; None if the target is gone.
fn on_handler<R: Send + 'static>(target: &str, f: impl FnOnce(Pin<&mut qobject::IpcHandler>) -> R + Send + 'static) -> Option<R> {
    let thread = handler_thread(target)?;
    let (tx, rx) = mpsc::channel();
    thread.queue(move |h| drop(tx.send(f(h)))).ok()?;
    rx.recv().ok()
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WireFunction {
    pub name: String,
    pub return_type: String,
    pub arguments: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WireTarget {
    pub name: String,
    pub functions: Vec<WireFunction>,
    pub properties: Vec<(String, String)>,
    /// (name, argument name, argument type); empty argument for signals without one.
    pub signals: Vec<(String, String, String)>,
}

impl WireFunction {
    fn write(&self, w: &mut Writer) {
        w.string(&self.name).string(&self.return_type);
        w.list(&self.arguments, |w, (n, t)| { w.string(n).string(t); });
    }
    fn read(r: &mut Reader) -> Result<Self, Incomplete> {
        Ok(Self { name: r.string()?, return_type: r.string()?, arguments: r.list(|r| Ok((r.string()?, r.string()?)))? })
    }
    pub fn to_text(&self) -> String {
        let args: Vec<String> = self.arguments.iter().map(|(n, t)| format!("{n}: {t}")).collect();
        format!("function {}({}): {}", self.name, args.join(", "), self.return_type)
    }
}

impl WireTarget {
    fn write(&self, w: &mut Writer) {
        w.string(&self.name);
        w.list(&self.functions, |w, f| f.write(w));
        w.list(&self.properties, |w, (n, t)| { w.string(n).string(t); });
        w.list(&self.signals, |w, (n, rn, rt)| { w.string(n).string(rn).string(rt); });
    }
    fn read(r: &mut Reader) -> Result<Self, Incomplete> {
        Ok(Self {
            name: r.string()?,
            functions: r.list(WireFunction::read)?,
            properties: r.list(|r| Ok((r.string()?, r.string()?)))?,
            signals: r.list(|r| Ok((r.string()?, r.string()?, r.string()?)))?,
        })
    }
    pub fn to_text(&self) -> String {
        let mut s = format!("target {}", self.name);
        for f in &self.functions {
            s += &format!("\n  {}", f.to_text());
        }
        for (n, t) in &self.properties {
            s += &format!("\n  property {n}: {t}");
        }
        for (n, rn, rt) in &self.signals {
            s += &if rt.is_empty() { format!("\n  signal {n}()") } else { format!("\n  signal {n}({rn}: {rt})") };
        }
        s
    }
}

fn describe(target: &str) -> Option<WireTarget> {
    let name = target.to_string();
    let lines: Vec<String> = on_handler(target, |h| qobject::ipc_describe(&h).iter().map(|l| l.to_string()).collect())?;
    let mut def = WireTarget { name, ..Default::default() };
    for line in lines {
        let f: Vec<String> = line.split('\u{1f}').map(String::from).collect();
        match f.first().map(String::as_str) {
            Some("f") if f.len() >= 3 => def.functions.push(WireFunction {
                name: f[1].clone(),
                return_type: f[2].clone(),
                arguments: f[3..].chunks(2).filter(|c| c.len() == 2).map(|c| (c[0].clone(), c[1].clone())).collect(),
            }),
            Some("p") if f.len() == 3 => def.properties.push((f[1].clone(), f[2].clone())),
            Some("s") if f.len() >= 2 => def.signals.push((f[1].clone(), f.get(2).cloned().unwrap_or_default(), f.get(3).cloned().unwrap_or_default())),
            _ => {}
        }
    }
    Some(def)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Kill,
    Query { target: String, name: String },
    Call { target: String, function: String, args: Vec<String> },
    Listen { target: String, signal: String },
    Property { target: String, property: String },
    /// qs-rs only (`qs frames`): an action for the runtime's frame meter, answered by the runtime
    /// itself, not by a shell target.
    Frames { action: String, arg: String },
}

/// The runtime's answer to `Command::Frames` (set by the qs-rs binary at launch).
pub static FRAMES_HOOK: std::sync::OnceLock<fn(&str, &str) -> String> = std::sync::OnceLock::new();

impl Command {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        match self {
            Command::Kill => { w.u8(1); },
            Command::Query { target, name } => { w.u8(2).string(target).string(name); },
            Command::Call { target, function, args } => {
                w.u8(3).string(target).string(function);
                w.list(args, |w, a| { w.string(a); });
            }
            Command::Listen { target, signal } => { w.u8(4).string(target).string(signal); },
            Command::Property { target, property } => { w.u8(5).string(target).string(property); },
            Command::Frames { action, arg } => { w.u8(6).string(action).string(arg); },
        }
        w.0
    }

    /// None for an invalid (monostate or unknown) command.
    fn decode(r: &mut Reader) -> Result<Option<Self>, Incomplete> {
        Ok(Some(match r.u8()? {
            1 => Command::Kill,
            2 => Command::Query { target: r.string()?, name: r.string()? },
            3 => Command::Call { target: r.string()?, function: r.string()?, args: r.list(|r| r.string())? },
            4 => Command::Listen { target: r.string()?, signal: r.string()? },
            5 => Command::Property { target: r.string()?, property: r.string()? },
            6 => Command::Frames { action: r.string()?, arg: r.string()? },
            _ => return Ok(None),
        }))
    }
}

// Response variant indices shared by every command: 1 not ready, 2 target not found, 3 entry
// not found; the command's own results follow.
const TARGET_NOT_FOUND: u8 = 2;
const ENTRY_NOT_FOUND: u8 = 3;

fn respond(cmd: Command) -> Option<Vec<u8>> {
    let mut w = Writer::default();
    match cmd {
        Command::Kill => {
            quickshell::qs_log!(warn, "quickshell.ipc", "Exiting due to IPC request.");
            qobject::ipc_quit();
            return None;
        }
        Command::Query { target, name } => {
            if target.is_empty() {
                let mut targets: Vec<String> = REGISTRY.lock().unwrap().as_ref().map(|m| m.keys().cloned().collect()).unwrap_or_default();
                targets.sort();
                let defs: Vec<WireTarget> = targets.iter().filter_map(|t| describe(t)).collect();
                w.u8(4).list(&defs, |w, d| d.write(w));
            } else if let Some(def) = describe(&target) {
                if name.is_empty() {
                    w.u8(5);
                    def.write(&mut w);
                } else if let Some(f) = def.functions.iter().find(|f| f.name == name) {
                    w.u8(6);
                    f.write(&mut w);
                } else if let Some((n, t)) = def.properties.iter().find(|(n, _)| *n == name) {
                    w.u8(7).string(n).string(t);
                } else {
                    w.u8(ENTRY_NOT_FOUND);
                }
            } else {
                w.u8(TARGET_NOT_FOUND);
            }
        }
        Command::Call { target, function, args } => match describe(&target) {
            None => { w.u8(TARGET_NOT_FOUND); },
            Some(def) => match def.functions.into_iter().find(|f| f.name == function) {
                None => { w.u8(ENTRY_NOT_FOUND); },
                Some(f) if f.arguments.len() != args.len() => {
                    w.u8(4);
                    f.write(&mut w);
                    w.bool(true).u8(0);
                }
                Some(f) => {
                    let name = function.clone();
                    let result = on_handler(&target, move |h| {
                        let mut out = QString::default();
                        let list: QStringList = args.iter().map(|a| QString::from(a.as_str())).collect();
                        let code = qobject::ipc_call(h, &QString::from(&name), &list, &mut out);
                        (code, out.to_string())
                    });
                    match result {
                        None => { w.u8(TARGET_NOT_FOUND); },
                        Some((0, value)) => { w.u8(5).bool(f.return_type == "void").string(&value); },
                        Some((-1, _)) => { w.u8(ENTRY_NOT_FOUND); },
                        Some((code, _)) => {
                            w.u8(4);
                            f.write(&mut w);
                            w.bool(code == -2).u8(if code > 0 { (code - 1) as u8 } else { 0 });
                        }
                    }
                }
            },
        },
        Command::Property { target, property } => {
            let result = on_handler(&target, move |h| {
                let mut out = QString::default();
                qobject::ipc_read_property(&h, &QString::from(&property), &mut out).then(|| out.to_string())
            });
            match result {
                None => { w.u8(TARGET_NOT_FOUND); },
                Some(None) => { w.u8(ENTRY_NOT_FOUND); },
                Some(Some(v)) => { w.u8(4).string(&v); },
            }
        }
        Command::Frames { action, arg } => match FRAMES_HOOK.get() {
            Some(hook) => { w.u8(4).string(&hook(&action, &arg)); },
            None => { w.u8(TARGET_NOT_FOUND); },
        },
        Command::Listen { .. } => unreachable!("served by listen()"),
    }
    Some(w.0)
}

fn serve(mut stream: UnixStream) {
    static NEXT_CONN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let conn = NEXT_CONN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    serve_commands(conn, &mut stream);
    LISTENERS.lock().unwrap().retain(|(c, ..)| *c != conn);
}

fn serve_commands(conn: u64, stream: &mut UnixStream) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        loop {
            let mut r = Reader::new(&buf);
            let cmd = match Command::decode(&mut r) {
                Err(Incomplete) => break,
                Ok(cmd) => cmd,
            };
            buf.drain(..r.pos);
            let Some(cmd) = cmd else {
                quickshell::qs_log!(warn, "quickshell.ipc", "received an invalid command");
                return;
            };
            let reply = match cmd {
                Command::Listen { target, signal } => listen(conn, stream, target, signal),
                cmd => respond(cmd),
            };
            if let Some(reply) = reply
                && stream.write_all(&reply).is_err() {
                    return;
                }
        }
    }
}

/// Client side (`qs-rs ipc ...`): sends one command to an instance and reads one reply with
/// `read`, which gets the reply's variant index and the rest of the bytes.
pub fn request<T>(instance_id: &str, cmd: &Command, read: impl Fn(u8, &mut Reader) -> Result<T, Incomplete>) -> std::io::Result<T> {
    let mut stream = UnixStream::connect(quickshell::instance::ipc_path(instance_id))?;
    stream.write_all(&cmd.encode())?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed"));
        }
        buf.extend_from_slice(&chunk[..n]);
        let mut r = Reader::new(&buf);
        if let Ok(v) = r.u8().and_then(|i| read(i, &mut r)) {
            return Ok(v);
        }
    }
}

/// Like `request`, for commands answered by a stream of replies (`ipc listen`): `on_reply` gets
/// each one and returns whether to keep reading.
pub fn stream<T>(
    instance_id: &str,
    cmd: &Command,
    read: impl Fn(u8, &mut Reader) -> Result<T, Incomplete>,
    mut on_reply: impl FnMut(T) -> bool,
) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(quickshell::instance::ipc_path(instance_id))?;
    stream.write_all(&cmd.encode())?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed"));
        }
        buf.extend_from_slice(&chunk[..n]);
        loop {
            let mut r = Reader::new(&buf);
            let Ok(v) = r.u8().and_then(|i| read(i, &mut r)) else { break };
            buf.drain(..r.pos);
            if !on_reply(v) {
                return Ok(());
            }
        }
    }
}

/// `qs kill`: asks the instance to quit and waits for it to close the socket.
pub fn kill(instance_id: &str, timeout: std::time::Duration) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(quickshell::instance::ipc_path(instance_id))?;
    stream.write_all(&Command::Kill.encode())?;
    stream.set_read_timeout(Some(timeout))?;
    let mut chunk = [0u8; 256];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => return Ok(()),
            Err(e) => return Err(e),
        }
    }
}

/// Reads a `prop get` reply: the value, or the error message.
pub fn read_property_reply(index: u8, r: &mut Reader) -> Result<Result<String, String>, Incomplete> {
    Ok(match index {
        4 => Ok(r.string()?),
        TARGET_NOT_FOUND => Err("Target not found.".into()),
        ENTRY_NOT_FOUND => Err("Property not found.".into()),
        1 => Err("Not ready to accept queries yet.".into()),
        _ => Err("Received invalid IPC response.".into()),
    })
}

/// Reads one `listen`/`wait` reply: a signal's value, or the error message.
pub fn read_listen_reply(index: u8, r: &mut Reader) -> Result<Result<String, String>, Incomplete> {
    Ok(match index {
        4 => Ok(r.string()?),
        TARGET_NOT_FOUND => Err("Target not found.".into()),
        ENTRY_NOT_FOUND => Err("Signal not found.".into()),
        1 => Err("Not ready to accept queries yet.".into()),
        _ => Err("Received invalid IPC response.".into()),
    })
}

/// Reads a call reply: Ok(Some(text)) for a value, Ok(None) for void, Err(message) otherwise.
pub fn read_call_reply(index: u8, r: &mut Reader, args: usize) -> Result<Result<Option<String>, String>, Incomplete> {
    Ok(match index {
        5 => {
            let is_void = r.bool()?;
            let value = r.string()?;
            Ok((!is_void).then_some(value))
        }
        4 => {
            let def = WireFunction::read(r)?;
            let count = r.bool()?;
            let i = r.u8()? as usize;
            let msg = if count {
                let n = def.arguments.len();
                format!("Too {} arguments provided ({n} required but {args} were provided.)", if n < args { "many" } else { "few" })
            } else {
                format!("Unable to parse argument {} as {}.", i + 1, def.arguments.get(i).map(|a| a.1.as_str()).unwrap_or("?"))
            };
            Err(format!("{msg}\nFunction definition: {}", def.to_text()))
        }
        TARGET_NOT_FOUND => Err("Target not found.".into()),
        ENTRY_NOT_FOUND => Err("Function not found.".into()),
        1 => Err("Not ready to accept queries yet.".into()),
        _ => Err("Received invalid IPC response.".into()),
    })
}

/// Reads a query reply as text (`ipc show`).
pub fn read_query_reply(index: u8, r: &mut Reader) -> Result<Result<String, String>, Incomplete> {
    Ok(match index {
        4 => Ok(r.list(WireTarget::read)?.iter().map(WireTarget::to_text).collect::<Vec<_>>().join("\n")),
        5 => Ok(WireTarget::read(r)?.to_text()),
        6 => Ok(WireFunction::read(r)?.to_text()),
        7 => Ok(format!("property {}: {}", r.string()?, r.string()?)),
        TARGET_NOT_FOUND => Err("Target not found.".into()),
        ENTRY_NOT_FOUND => Err("Function not found.".into()),
        1 => Err("Not ready to accept queries yet.".into()),
        _ => Err("Received invalid IPC response.".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_command_bytes() {
        let cmd = Command::Call { target: "t".into(), function: "f".into(), args: vec!["a".into()] };
        let bytes = cmd.encode();
        assert_eq!(bytes, [3, 0, 0, 0, 2, 0, b't', 0, 0, 0, 2, 0, b'f', 0, 0, 0, 1, 0, 0, 0, 2, 0, b'a']);
        assert_eq!(Command::decode(&mut Reader::new(&bytes)), Ok(Some(cmd)));
        assert_eq!(Command::decode(&mut Reader::new(&bytes[..bytes.len() - 1])), Err(Incomplete));
    }

    #[test]
    fn target_round_trips() {
        let def = WireTarget {
            name: "iris".into(),
            functions: vec![WireFunction { name: "toggle".into(), return_type: "void".into(), arguments: vec![("x".into(), "int".into())] }],
            properties: vec![("open".into(), "bool".into())],
            signals: vec![("opened".into(), String::new(), String::new())],
        };
        let mut w = Writer::default();
        def.write(&mut w);
        assert_eq!(WireTarget::read(&mut Reader::new(&w.0)).unwrap(), def);
        assert_eq!(def.to_text(), "target iris\n  function toggle(x: int): void\n  property open: bool\n  signal opened()");
    }
}
