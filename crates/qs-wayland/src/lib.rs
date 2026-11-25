//! Wayland protocols Qt does not bind, spoken by Rust on Qt's own connection. The connection
//! wraps Qt's wl_display (guest mode: Qt keeps owning it); each protocol gets its own event
//! queue, and all queues share ONE `qs-wayland` dispatcher thread using libwayland's
//! `prepare_read` / `read_events` protocol and `poll(2)`, reporting to QML through
//! `CxxQtThread::queue`.

extern crate cxx_qt_lib;
mod qt;

pub use wayland_client;
pub use wayland_protocols;
pub use wayland_protocols_wlr;

use std::io::ErrorKind;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::{Arc, Mutex, OnceLock};
use wayland_client::backend::{ReadEventsGuard, WaylandError};
use wayland_client::{Connection, DispatchError, EventQueue};

/// Qt's Wayland connection. The first call must happen on the Qt thread, after the
/// QGuiApplication exists; `None` when Qt is not on Wayland.
pub fn connection() -> Option<Connection> {
    static CONN: OnceLock<Option<Connection>> = OnceLock::new();
    CONN.get_or_init(|| {
        let display = qt::ffi::qt_wl_display();
        if display == 0 {
            return None;
        }
        // SAFETY: Qt owns this display for the life of the application, which outlives us.
        let backend = unsafe { wayland_backend::client::Backend::from_foreign_display(display as *mut _) };
        Some(Connection::from_backend(backend))
    })
    .clone()
}

/// Name of the QScreen showing this wl_output (one of Qt's own proxies, as received in events
/// that carry outputs), or an empty string. Qt thread only.
pub fn screen_name_for_output(output: &wayland_client::protocol::wl_output::WlOutput) -> String {
    use wayland_client::Proxy;
    qt::ffi::screen_name_for_output(output.id().as_ptr() as usize).to_string()
}

/// Qt's wl_output proxy for the screen named `name`. Qt thread only.
pub fn output_for_screen_name(name: &str) -> Option<wayland_client::protocol::wl_output::WlOutput> {
    use wayland_client::Proxy;
    let ptr = qt::ffi::output_for_screen_name(&cxx_qt_lib::QString::from(name));
    if ptr == 0 {
        return None;
    }
    // SAFETY: Qt keeps this wl_output proxy alive while its QScreen exists.
    let id = unsafe {
        wayland_client::backend::ObjectId::from_ptr(wayland_client::protocol::wl_output::WlOutput::interface(), ptr as *mut _)
    }
    .ok()?;
    wayland_client::protocol::wl_output::WlOutput::from_id(&connection()?, id).ok()
}

trait DynQueue: Send {
    fn name(&self) -> &str;
    fn dispatch_pending(&mut self) -> Result<usize, DispatchError>;
    fn prepare_read(&self) -> Option<ReadEventsGuard>;
}

struct RegisteredQueue<S> {
    name: String,
    queue: EventQueue<S>,
    state: S,
}

impl<S: Send + 'static> DynQueue for RegisteredQueue<S> {
    fn name(&self) -> &str {
        &self.name
    }

    fn dispatch_pending(&mut self) -> Result<usize, DispatchError> {
        self.queue.dispatch_pending(&mut self.state)
    }

    fn prepare_read(&self) -> Option<ReadEventsGuard> {
        self.queue.prepare_read()
    }
}

struct Dispatcher {
    pending: Mutex<Vec<Box<dyn DynQueue>>>,
    wake: OwnedFd,
}

fn dispatcher() -> &'static Arc<Dispatcher> {
    static D: OnceLock<Arc<Dispatcher>> = OnceLock::new();
    D.get_or_init(|| {
        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        assert!(fd >= 0, "eventfd failed");
        let wake = unsafe { OwnedFd::from_raw_fd(fd) };
        let d = Arc::new(Dispatcher { pending: Mutex::new(Vec::new()), wake });
        let thread_d = Arc::clone(&d);
        let conn = connection().expect("wayland connection before spawn_dispatch");
        std::thread::Builder::new()
            .name("qs-wayland".into())
            .spawn(move || run_dispatcher(thread_d, conn))
            .expect("spawn qs-wayland thread");
        d
    })
}

fn drain_eventfd(fd: libc::c_int) {
    let mut buf = 0u64;
    unsafe { libc::read(fd, (&raw mut buf).cast(), 8) };
}

fn run_dispatcher(d: Arc<Dispatcher>, conn: Connection) {
    let mut queues: Vec<Box<dyn DynQueue>> = Vec::new();
    let wake_fd = d.wake.as_raw_fd();

    'outer: loop {
        {
            let mut pending = d.pending.lock().unwrap();
            if !pending.is_empty() {
                queues.append(&mut pending);
            }
        }

        if queues.is_empty() {
            let mut pfd = libc::pollfd { fd: wake_fd, events: libc::POLLIN, revents: 0 };
            let r = unsafe { libc::poll(&raw mut pfd, 1, -1) };
            if r > 0 {
                drain_eventfd(wake_fd);
            }
            continue;
        }

        // Drain all already-queued events across every protocol queue before sleeping.
        loop {
            let mut dispatched = 0usize;
            let mut i = 0;
            while i < queues.len() {
                match queues[i].dispatch_pending() {
                    Ok(n) => {
                        dispatched += n;
                        i += 1;
                    }
                    Err(e) => {
                        let q = queues.swap_remove(i);
                        quickshell::qs_log!(warn, "quickshell.wayland", "{}: wayland queue stopped: {e}", q.name());
                    }
                }
            }
            if queues.is_empty() {
                continue 'outer;
            }
            if dispatched == 0 {
                break;
            }
        }

        match conn.flush() {
            Ok(()) => {}
            Err(WaylandError::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => {
                quickshell::qs_log!(warn, "quickshell.wayland", "qs-wayland: connection flush failed: {e}");
                return;
            }
        }

        // Hold ONE ReadEventsGuard on queues[0] so libwayland's reader_count >= 1 blocks any
        // other thread (e.g. Qt's WaylandEventThread) from completing wl_display_read_events()
        // while we verify the remaining queues are also empty.
        let Some(guard) = queues[0].prepare_read() else {
            continue 'outer;
        };

        for q in &queues[1..] {
            match q.prepare_read() {
                // Probe increments reader_count >= 2 and immediately cancels back to >= 1.
                Some(probe) => drop(probe),
                None => {
                    drop(guard);
                    continue 'outer;
                }
            }
        }

        let mut pfds = [
            libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: wake_fd, events: libc::POLLIN, revents: 0 },
        ];
        let rc = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, -1) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            drop(guard);
            if err.kind() == ErrorKind::Interrupted {
                continue 'outer;
            }
            quickshell::qs_log!(warn, "quickshell.wayland", "qs-wayland: poll failed: {err}");
            return;
        }

        let conn_ready = (pfds[0].revents & (libc::POLLIN | libc::POLLERR | libc::POLLHUP)) != 0;
        let wake_ready = (pfds[1].revents & libc::POLLIN) != 0;

        if wake_ready {
            drain_eventfd(wake_fd);
        }

        if conn_ready {
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => {
                    quickshell::qs_log!(warn, "quickshell.wayland", "qs-wayland: read failed: {e}");
                    return;
                }
            }
        } else {
            // Woken only by a new queue registration; cancel_read and pick it up.
            drop(guard);
        }
    }
}

/// Registers `queue` on the shared `qs-wayland` dispatcher thread for the rest of the program.
pub fn spawn_dispatch<S: Send + 'static>(name: &str, queue: EventQueue<S>, state: S) {
    let d = dispatcher();
    d.pending.lock().unwrap().push(Box::new(RegisteredQueue { name: name.into(), queue, state }));
    let one: u64 = 1;
    unsafe { libc::write(d.wake.as_raw_fd(), (&raw const one).cast(), 8) };
}

#[cfg(test)]
mod link_init {
    #[test]
    fn init() {
        cxx_qt::init_crate!(cxx_qt);
        cxx_qt::init_crate!(cxx_qt_lib);
    }
}
