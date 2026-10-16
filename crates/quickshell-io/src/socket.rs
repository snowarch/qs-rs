//! `Socket`: a unix stream socket client feeding a stream parser. A reader thread queues data
//! back to the Qt thread and a writer thread drains `write()`s; a connection id drops events
//! from a previous connection. Semantics follow quickshell's Socket over QLocalSocket.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, connected, READ, WRITE = set_connected, NOTIFY = connection_state_changed)]
        #[qproperty(QString, path, READ, WRITE = set_path, NOTIFY)]
        #[qproperty(*mut QObject, parser, READ, WRITE = set_parser, NOTIFY)]
        type Socket = super::SocketRust;

        fn set_connected(self: Pin<&mut Socket>, connected: bool);
        fn set_path(self: Pin<&mut Socket>, path: QString);
        unsafe fn set_parser(self: Pin<&mut Socket>, parser: *mut QObject);

        #[qsignal]
        #[cxx_name = "connectionStateChanged"]
        fn connection_state_changed(self: Pin<&mut Socket>);
        /// QLocalSocket::LocalSocketError values.
        #[qsignal]
        fn error(self: Pin<&mut Socket>, error: i32);

        #[qinvokable]
        fn write(self: Pin<&mut Socket>, data: &QString);
        #[qinvokable]
        fn flush(self: Pin<&mut Socket>);
    }

    impl cxx_qt::Threading for Socket {}
}

use crate::datastream::Parser;
use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use std::io::{ErrorKind, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;

pub struct SocketRust {
    connected: bool,
    path: QString,
    parser: *mut cxx_qt::QObject,
    buffer: Vec<u8>,
    conn_id: u64,
    /// A connection attempt or connection exists.
    active: bool,
    target_connected: bool,
    disconnecting: bool,
    stream: Option<UnixStream>,
    writer: Option<mpsc::Sender<Vec<u8>>>,
}

impl Default for SocketRust {
    fn default() -> Self {
        Self {
            connected: false,
            path: QString::default(),
            parser: std::ptr::null_mut(),
            buffer: Vec::new(),
            conn_id: 0,
            active: false,
            target_connected: false,
            disconnecting: false,
            stream: None,
            writer: None,
        }
    }
}

enum Event {
    Connected(UnixStream),
    Failed(i32),
    Data(Vec<u8>),
    Closed,
}

fn error_code(kind: ErrorKind) -> i32 {
    match kind {
        ErrorKind::ConnectionRefused => 0,
        ErrorKind::BrokenPipe | ErrorKind::ConnectionReset => 1,
        ErrorKind::NotFound => 2,
        ErrorKind::PermissionDenied => 3,
        _ => -1,
    }
}

impl qobject::Socket {
    fn connect_path(mut self: Pin<&mut Self>) {
        if self.path.is_empty() {
            return;
        }
        let mut rust = self.as_mut().rust_mut();
        rust.conn_id += 1;
        rust.active = true;
        let (id, path) = (rust.conn_id, rust.path.to_string());
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let send = |e: Event| qt.queue(move |this| this.on_event(id, e)).is_ok();
            let stream = match UnixStream::connect(&path) {
                Ok(s) => s,
                Err(e) => {
                    send(Event::Failed(error_code(e.kind())));
                    return;
                }
            };
            let Ok(mut reader) = stream.try_clone() else {
                send(Event::Failed(-1));
                return;
            };
            if !send(Event::Connected(stream)) {
                return;
            }
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if !send(Event::Data(chunk[..n].to_vec())) {
                            return;
                        }
                    }
                }
            }
            send(Event::Closed);
        });
    }

    fn on_event(mut self: Pin<&mut Self>, id: u64, event: Event) {
        if id != self.conn_id {
            if let Event::Connected(s) = event {
                let _ = s.shutdown(Shutdown::Both);
            }
            return;
        }
        match event {
            Event::Connected(stream) => {
                let (tx, rx) = mpsc::channel::<Vec<u8>>();
                if let Ok(mut w) = stream.try_clone() {
                    std::thread::spawn(move || {
                        for data in rx {
                            if w.write_all(&data).is_err() {
                                break;
                            }
                        }
                    });
                }
                let mut rust = self.as_mut().rust_mut();
                rust.buffer.clear();
                rust.stream = Some(stream);
                rust.writer = Some(tx);
                rust.connected = true;
                rust.target_connected = false;
                rust.disconnecting = false;
                self.as_mut().connection_state_changed();
            }
            Event::Failed(code) => {
                let mut rust = self.as_mut().rust_mut();
                rust.active = false;
                rust.disconnecting = false;
                self.as_mut().error(code);
            }
            Event::Data(bytes) => {
                let mut buf = std::mem::take(&mut self.as_mut().rust_mut().buffer);
                if let Some(p) = unsafe { Parser::from_ptr(self.parser) } {
                    p.feed(&mut buf, &bytes);
                }
                self.as_mut().rust_mut().buffer = buf;
            }
            Event::Closed => {
                let mut rust = self.as_mut().rust_mut();
                let was_connected = rust.connected;
                let by_us = rust.disconnecting;
                rust.connected = false;
                rust.disconnecting = false;
                rust.active = false;
                rust.stream = None;
                rust.writer = None;
                rust.buffer.clear();
                if !was_connected {
                    return;
                }
                if !by_us {
                    self.as_mut().error(1);
                }
                self.as_mut().connection_state_changed();
                if self.target_connected {
                    self.connect_path();
                }
            }
        }
    }

    fn set_connected(mut self: Pin<&mut Self>, connected: bool) {
        self.as_mut().rust_mut().target_connected = connected;
        if !connected {
            if self.active && !self.disconnecting {
                let mut rust = self.as_mut().rust_mut();
                rust.disconnecting = true;
                match &rust.stream {
                    Some(s) => {
                        let _ = s.shutdown(Shutdown::Both);
                    }
                    // Still connecting: forget the attempt.
                    None => {
                        rust.conn_id += 1;
                        rust.active = false;
                        rust.disconnecting = false;
                    }
                }
            }
        } else if !self.active {
            self.connect_path();
        }
    }

    fn set_path(mut self: Pin<&mut Self>, path: QString) {
        if (self.connected && !self.disconnecting) || path == self.path {
            return;
        }
        self.as_mut().rust_mut().path = path;
        self.as_mut().path_changed();
        if self.target_connected && !self.active {
            self.connect_path();
        }
    }

    unsafe fn set_parser(mut self: Pin<&mut Self>, parser: *mut cxx_qt::QObject) {
        if self.parser != parser {
            self.as_mut().rust_mut().parser = parser;
            self.parser_changed();
        }
    }

    fn write(self: Pin<&mut Self>, data: &QString) {
        if let Some(w) = &self.writer {
            let _ = w.send(data.to_string().into_bytes());
        }
    }

    /// Writes are handed to the writer thread right away, so there is nothing to flush.
    fn flush(self: Pin<&mut Self>) {}
}

impl Drop for SocketRust {
    fn drop(&mut self) {
        if let Some(s) = &self.stream {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
}
