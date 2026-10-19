//! The C++ side of window capture (include/windowcast.h, include/dmabuf.h) and the frame queue it
//! reads from windowcast.rs. Its own bridge: windowcast.h includes this header, which a bridge
//! with a QObject cannot be.

#[cxx_qt::bridge]
pub mod ffi {
    #[namespace = "qsdmabuf"]
    unsafe extern "C++" {
        include!("quickshell_wayland/include/dmabuf.h");
        /// Modifiers EGL can import as a 2D texture for this DRM fourcc, plus
        /// DRM_FORMAT_MOD_INVALID when implicit modifiers work; empty without EGL.
        fn dmabuf_modifiers(fourcc: u32) -> Vec<u64>;
    }

    /// One frame handed to the render thread. `fds` are duplicates the caller closes.
    struct CastFrame {
        key: u64,
        width: u32,
        height: u32,
        fourcc: u32,
        modifier: u64,
        planes: u32,
        fds: [i32; 4],
        offsets: [u32; 4],
        strides: [u32; 4],
    }

    extern "Rust" {
        /// Takes the newest frame not taken yet; the item owns its buffer until `cast_release`.
        unsafe fn cast_take(frames: usize, out: &mut CastFrame) -> bool;
        /// Gives a taken buffer back to the compositor.
        unsafe fn cast_release(frames: usize, key: u64);
        /// Whether the stream still has the buffer behind `key` (a resize replaces them all).
        unsafe fn cast_alive(frames: usize, key: u64) -> bool;
        unsafe fn cast_retain(frames: usize);
        unsafe fn cast_unref(frames: usize);
    }

}

use crate::windowcast::{cast_alive, cast_release, cast_retain, cast_take, cast_unref};
