//! `Socket`: a unix stream socket client feeding a stream parser.
//!
//! One shared `poll(2)` reactor thread ("qs-sock") drives every live socket (connecting, reading
//! into a single reused 64 KB buffer, draining writes, and reporting closes back to the Qt thread).
//! Semantics follow quickshell's Socket over QLocalSocket.

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
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::Shutdown;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

type Emit = Box<dyn Fn(Event) -> bool + Send>;

enum Event {
    Connected,
    Failed(i32),
    Data(Vec<u8>),
    Closed,
}

enum Cmd {
    Connect { id: u64, path: String, emit: Emit },
    Write { id: u64, data: Vec<u8> },
    Disconnect { id: u64, notify: bool },
}

struct Conn {
    stream: UnixStream,
    pending: Vec<u8>,
    emit: Emit,
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
        std::thread::Builder::new().name("qs-sock".into()).spawn(run_reactor).expect("socket reactor thread");
        Reactor { cmds: Mutex::new(Vec::new()), wake, next: AtomicU64::new(1) }
    })
}

impl Reactor {
    fn next_id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    fn send(&self, cmd: Cmd) {
        self.cmds.lock().unwrap().push(cmd);
        let one: u64 = 1;
        unsafe { libc::write(self.wake.as_raw_fd(), (&raw const one).cast(), 8) };
    }
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

fn run_reactor() {
    let r = reactor();
    let mut conns: HashMap<u64, Conn> = HashMap::new();
    let mut fds: Vec<libc::pollfd> = Vec::new();
    let mut ids: Vec<u64> = Vec::new();
    let mut read_buf = vec![0u8; 64 * 1024];
    let mut closed: Vec<(u64, bool)> = Vec::new();

    loop {
        for cmd in std::mem::take(&mut *r.cmds.lock().unwrap()) {
            match cmd {
                Cmd::Connect { id, path, emit } => match UnixStream::connect(&path) {
                    Ok(stream) => {
                        if stream.set_nonblocking(true).is_err() {
                            let _ = emit(Event::Failed(-1));
                            continue;
                        }
                        if emit(Event::Connected) {
                            conns.insert(id, Conn { stream, pending: Vec::new(), emit });
                        } else {
                            let _ = stream.shutdown(Shutdown::Both);
                        }
                    }
                    Err(e) => {
                        let _ = emit(Event::Failed(error_code(e.kind())));
                    }
                },
                Cmd::Write { id, data } => {
                    if let Some(c) = conns.get_mut(&id) {
                        c.pending.extend_from_slice(&data);
                    }
                }
                Cmd::Disconnect { id, notify } => {
                    if let Some(c) = conns.remove(&id) {
                        let _ = c.stream.shutdown(Shutdown::Both);
                        if notify {
                            let _ = (c.emit)(Event::Closed);
                        }
                    }
                }
            }
        }

        fds.clear();
        ids.clear();
        fds.push(libc::pollfd { fd: r.wake.as_raw_fd(), events: libc::POLLIN, revents: 0 });
        ids.push(0);
        for (&id, c) in &conns {
            let mut events = libc::POLLIN;
            if !c.pending.is_empty() {
                events |= libc::POLLOUT;
            }
            fds.push(libc::pollfd { fd: c.stream.as_raw_fd(), events, revents: 0 });
            ids.push(id);
        }

        if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) } < 0 {
            continue;
        }

        closed.clear();
        for (pfd, &id) in fds.iter().zip(&ids) {
            if pfd.revents == 0 {
                continue;
            }
            if id == 0 {
                let mut v = 0u64;
                unsafe { libc::read(r.wake.as_raw_fd(), (&raw mut v).cast(), 8) };
                continue;
            }
            let Some(c) = conns.get_mut(&id) else { continue };
            let mut dead = false;
            let mut notify = true;

            if pfd.revents & libc::POLLOUT != 0 && !c.pending.is_empty() {
                match c.stream.write(&c.pending) {
                    Ok(0) => dead = true,
                    Ok(n) => {
                        c.pending.drain(..n);
                    }
                    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                    Err(_) => dead = true,
                }
            }

            if !dead && (pfd.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0) {
                loop {
                    match c.stream.read(&mut read_buf) {
                        Ok(0) => {
                            dead = true;
                            break;
                        }
                        Ok(n) => {
                            if !(c.emit)(Event::Data(read_buf[..n].to_vec())) {
                                dead = true;
                                notify = false;
                                break;
                            }
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                        Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                        Err(_) => {
                            dead = true;
                            break;
                        }
                    }
                }
            }

            if dead {
                closed.push((id, notify));
            }
        }

        for (id, notify) in closed.drain(..) {
            if let Some(c) = conns.remove(&id) {
                let _ = c.stream.shutdown(Shutdown::Both);
                if notify {
                    let _ = (c.emit)(Event::Closed);
                }
            }
        }
    }
}

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
        }
    }
}

impl qobject::Socket {
    fn connect_path(mut self: Pin<&mut Self>) {
        if self.path.is_empty() {
            return;
        }
        let id = reactor().next_id();
        let path = {
            let mut rust = self.as_mut().rust_mut();
            rust.conn_id = id;
            rust.active = true;
            rust.path.to_string()
        };
        let qt = self.qt_thread();
        let emit: Emit = Box::new(move |event| qt.queue(move |this| this.on_event(id, event)).is_ok());
        reactor().send(Cmd::Connect { id, path, emit });
    }

    fn on_event(mut self: Pin<&mut Self>, id: u64, event: Event) {
        if id != self.conn_id {
            if matches!(event, Event::Connected) {
                reactor().send(Cmd::Disconnect { id, notify: false });
            }
            return;
        }
        match event {
            Event::Connected => {
                let mut rust = self.as_mut().rust_mut();
                rust.buffer.clear();
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
                let id = self.conn_id;
                let was_connected = self.connected;
                let mut rust = self.as_mut().rust_mut();
                if was_connected {
                    rust.disconnecting = true;
                    reactor().send(Cmd::Disconnect { id, notify: true });
                } else {
                    // Still connecting: forget the attempt.
                    rust.conn_id = reactor().next_id();
                    rust.active = false;
                    rust.disconnecting = false;
                    reactor().send(Cmd::Disconnect { id, notify: false });
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
        if self.connected && !self.disconnecting {
            reactor().send(Cmd::Write { id: self.conn_id, data: data.to_string().into_bytes() });
        }
    }

    /// Writes are queued onto the reactor right away, so there is nothing to flush.
    fn flush(self: Pin<&mut Self>) {}
}

impl Drop for SocketRust {
    fn drop(&mut self) {
        if self.active && self.conn_id != 0 {
            reactor().send(Cmd::Disconnect { id: self.conn_id, notify: false });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn one_reactor_thread_for_many_sockets() {
        let dir = std::env::temp_dir().join(format!("qsrs-sock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let srv = std::thread::spawn(move || {
            for stream in listener.incoming().take(6).flatten() {
                std::thread::spawn(move || {
                    let mut s = stream;
                    let mut buf = [0u8; 64];
                    if let Ok(n) = s.read(&mut buf) {
                        let _ = s.write_all(&buf[..n]);
                    }
                });
            }
        });

        let mut rxs = Vec::new();
        for i in 0..6 {
            let id = reactor().next_id();
            let (tx, rx) = mpsc::channel();
            let emit: Emit = Box::new(move |e| tx.send(e).is_ok());
            reactor().send(Cmd::Connect { id, path: path.display().to_string(), emit });
            rxs.push((id, i, rx));
        }
        for (id, i, rx) in &rxs {
            assert!(matches!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Event::Connected));
            reactor().send(Cmd::Write { id: *id, data: format!("msg{i}").into_bytes() });
        }
        for (_, i, rx) in &rxs {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                Event::Data(d) => assert_eq!(d, format!("msg{i}").as_bytes()),
                _ => panic!("expected Data"),
            }
        }
        let sock_threads = std::fs::read_dir("/proc/self/task")
            .unwrap()
            .flatten()
            .filter(|t| std::fs::read_to_string(t.path().join("comm")).is_ok_and(|c| c.trim() == "qs-sock"))
            .count();
        assert_eq!(sock_threads, 1);
        srv.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
