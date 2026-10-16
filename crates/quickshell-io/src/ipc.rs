//! `IpcHandler` and the IPC server behind `qs-rs ipc call`. Handlers register by target; the
//! server thread forwards each request to the handler's Qt thread and waits for the reply.
//!
//! Wire format: request fields separated by NUL (`call`, target, function, args... or `show`),
//! request ends when the client shuts down its write half. Reply: one status byte (`0` ok) then
//! UTF-8 text.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("quickshell_io/include/ipcmeta.h");
        #[namespace = "qsrs_io"]
        fn ipc_call(handler: Pin<&mut IpcHandler>, name: &QString, args: &QStringList, out: &mut QString) -> i32;
        #[namespace = "qsrs_io"]
        fn ipc_describe(handler: &IpcHandler) -> QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, enabled, READ, WRITE = set_enabled, NOTIFY)]
        #[qproperty(QString, target, READ, WRITE = set_target, NOTIFY)]
        type IpcHandler = super::IpcHandlerRust;

        fn set_enabled(self: Pin<&mut IpcHandler>, enabled: bool);
        fn set_target(self: Pin<&mut IpcHandler>, target: QString);
    }

    impl cxx_qt::Threading for IpcHandler {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::{QString, QStringList};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, mpsc};

type Thread = CxxQtThread<qobject::IpcHandler>;

/// target -> (registration id, handler thread handle)
static REGISTRY: Mutex<Option<HashMap<String, (u64, Thread)>>> = Mutex::new(None);
static NEXT_ID: Mutex<u64> = Mutex::new(0);
static SERVER: OnceLock<()> = OnceLock::new();

pub struct IpcHandlerRust {
    enabled: bool,
    target: QString,
    id: u64,
    registered: Option<String>,
}

impl Default for IpcHandlerRust {
    fn default() -> Self {
        let mut n = NEXT_ID.lock().unwrap();
        *n += 1;
        Self { enabled: true, target: QString::default(), id: *n, registered: None }
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
    if let Some(map) = reg.as_mut() {
        if map.get(target).is_some_and(|(rid, _)| *rid == id) {
            map.remove(target);
        }
    }
}

pub fn socket_path(shell_id: &str) -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("qs-rs/by-shell").join(shell_id).join("ipc.sock")
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
            let mut reg = REGISTRY.lock().unwrap();
            let map = reg.get_or_insert_with(HashMap::new);
            if map.contains_key(&target) {
                eprintln!("IpcHandler: target {target:?} is already registered, ignoring the duplicate.");
                return;
            }
            map.insert(target.clone(), (id, self.qt_thread()));
        }
        self.as_mut().rust_mut().registered = Some(target);
        SERVER.get_or_init(start_server);
    }
}

fn start_server() {
    let path = socket_path(&quickshell::common::shell().shell_id);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("IPC: could not listen on {}: {e}", path.display());
            return;
        }
    };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve(stream));
        }
    });
}

fn on_handler(target: &str, f: impl FnOnce(Pin<&mut qobject::IpcHandler>) -> (u8, String) + Send + 'static) -> (u8, String) {
    let thread = REGISTRY.lock().unwrap().as_ref().and_then(|m| m.get(target).map(|(_, t)| t.clone()));
    let Some(thread) = thread else { return (1, format!("Target {target} not found.")) };
    let (tx, rx) = mpsc::channel();
    if thread.queue(move |h| drop(tx.send(f(h)))).is_err() {
        return (1, format!("Target {target} is gone."));
    }
    rx.recv().unwrap_or((1, "No reply from handler.".into()))
}

fn serve(mut stream: UnixStream) {
    let mut buf = Vec::new();
    if stream.read_to_end(&mut buf).is_err() {
        return;
    }
    let fields: Vec<String> = buf.split(|b| *b == 0).map(|f| String::from_utf8_lossy(f).into_owned()).collect();
    let (status, text) = match fields.first().map(String::as_str) {
        Some("call") if fields.len() >= 3 => {
            let (target, name) = (fields[1].clone(), fields[2].clone());
            let args = fields[3..].to_vec();
            on_handler(&target, move |h| {
                let mut out = QString::default();
                let list: QStringList = args.iter().map(|a| QString::from(a.as_str())).collect();
                let code = qobject::ipc_call(h, &QString::from(&name), &list, &mut out);
                (code as u8, out.to_string())
            })
        }
        Some("show") => {
            let mut targets: Vec<String> = REGISTRY.lock().unwrap().as_ref().map(|m| m.keys().cloned().collect()).unwrap_or_default();
            targets.sort();
            let mut text = String::new();
            for t in targets {
                let (_, desc) = on_handler(&t.clone(), |h| (0, qobject::ipc_describe(&h).to_string()));
                text.push_str(&format!("target {t}\n{desc}"));
            }
            (0, text)
        }
        _ => (1, "Malformed request.".into()),
    };
    let _ = stream.write_all(&[status]);
    let _ = stream.write_all(text.as_bytes());
}

/// Client side, used by the `ipc` subcommand. Returns the status byte and reply text.
pub fn request(shell_id: &str, fields: &[String]) -> std::io::Result<(u8, String)> {
    let mut stream = UnixStream::connect(socket_path(shell_id))?;
    stream.write_all(fields.join("\0").as_bytes())?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf)?;
    let status = buf.first().copied().unwrap_or(1);
    Ok((status, String::from_utf8_lossy(buf.get(1..).unwrap_or_default()).into_owned()))
}
