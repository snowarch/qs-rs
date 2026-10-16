//! Hyprland IPC core: request socket (one connection per request, run off the GUI thread) and the
//! event socket stream. Inert when HYPRLAND_INSTANCE_SIGNATURE is not set (e.g. under niri).

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "HyprlandIpcCore"]
        #[qml_singleton]
        #[qproperty(bool, available, READ, CONSTANT)]
        #[qproperty(QString, request_socket_path, cxx_name = "requestSocketPath", READ, CONSTANT)]
        #[qproperty(QString, event_socket_path, cxx_name = "eventSocketPath", READ, CONSTANT)]
        type HyprlandIpcCore = super::HyprlandIpcRust;

        /// Sends `request` and answers through `reply(id, text)`.
        #[qinvokable]
        fn request(self: Pin<&mut HyprlandIpcCore>, id: i32, request: &QString);

        #[qsignal]
        fn reply(self: Pin<&mut HyprlandIpcCore>, id: i32, text: QString);
        #[qsignal]
        #[cxx_name = "rawEvent"]
        fn raw_event(self: Pin<&mut HyprlandIpcCore>, name: QString, data: QString);
    }

    impl cxx_qt::Initialize for HyprlandIpcCore {}
    impl cxx_qt::Threading for HyprlandIpcCore {}
}

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub struct HyprlandIpcRust {
    available: bool,
    request_socket_path: QString,
    event_socket_path: QString,
}

fn socket_dir() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok().filter(|s| !s.is_empty())?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    let dir = runtime.join("hypr").join(&sig);
    Some(if dir.exists() { dir } else { PathBuf::from("/tmp/hypr").join(sig) })
}

impl Default for HyprlandIpcRust {
    fn default() -> Self {
        let dir = socket_dir();
        let path = |name: &str| dir.as_ref().map(|d| QString::from(&*d.join(name).to_string_lossy())).unwrap_or_default();
        Self { available: dir.is_some(), request_socket_path: path(".socket.sock"), event_socket_path: path(".socket2.sock") }
    }
}

/// One request/response exchange on the request socket.
pub fn send(path: &str, request: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(path)?;
    stream.write_all(request.as_bytes())?;
    let mut out = String::new();
    stream.read_to_string(&mut out)?;
    Ok(out)
}

impl cxx_qt::Initialize for qobject::HyprlandIpcCore {
    fn initialize(self: Pin<&mut Self>) {
        if !self.available {
            return;
        }
        let path = self.event_socket_path.to_string();
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let Ok(stream) = UnixStream::connect(&path) else {
                eprintln!("Hyprland: could not connect to event socket {path}");
                return;
            };
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let (name, data) = line.split_once(">>").unwrap_or((&line, ""));
                let (name, data) = (QString::from(name), QString::from(data));
                if qt.queue(move |this| this.raw_event(name, data)).is_err() {
                    break;
                }
            }
        });
    }
}

impl qobject::HyprlandIpcCore {
    fn request(self: Pin<&mut Self>, id: i32, request: &QString) {
        if !self.available {
            return;
        }
        let (path, request) = (self.request_socket_path.to_string(), request.to_string());
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let text = send(&path, &request).unwrap_or_else(|e| {
                eprintln!("Hyprland: request {request:?} failed: {e}");
                String::new()
            });
            let _ = qt.queue(move |this| this.reply(id, QString::from(&text)));
        });
    }
}
