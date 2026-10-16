//! Wayland protocols Qt does not bind, spoken by Rust on Qt's own connection. The connection
//! wraps Qt's wl_display (guest mode: Qt keeps owning it); each protocol gets its own event
//! queue dispatched on its own thread, and reports to QML through `CxxQtThread::queue`.
//! libwayland's prepare_read/read_events protocol lets that thread and Qt's event thread read
//! the same socket.

extern crate cxx_qt_lib;
mod qt;

pub use wayland_client;
pub use wayland_protocols;
pub use wayland_protocols_wlr;

use std::sync::OnceLock;
use wayland_client::{Connection, EventQueue};

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

/// Dispatches `queue` on a new thread for the rest of the program.
pub fn spawn_dispatch<S: Send + 'static>(name: &str, mut queue: EventQueue<S>, mut state: S) {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            while let Ok(_) = queue.blocking_dispatch(&mut state) {}
            eprintln!("{}: wayland queue stopped", std::thread::current().name().unwrap_or("wayland"));
        })
        .expect("spawn wayland thread");
}

#[cfg(test)]
mod link_init {
    #[test]
    fn init() {
        cxx_qt::init_crate!(cxx_qt);
        cxx_qt::init_crate!(cxx_qt_lib);
    }
}
