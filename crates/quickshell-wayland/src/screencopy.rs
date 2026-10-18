//! ScreencopyView capture over wlr-screencopy-unstable-v1 (whole screens; niri 26.04 offers no
//! other capture protocol). One shared queue/thread holds the manager and wl_shm; each capture
//! asks for one frame at a time into shared-memory buffers it reuses, and hands every frame to
//! QML as a QImage over that memory (no copy; the buffer goes back to the pool when the last
//! QImage referencing it dies). Frames carry their context's generation so a frame for a
//! replaced screen or cursor setting never lands.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qimage.h");
        type QImage = cxx_qt_lib::QImage;
        include!("cxx-qt-lib/qsize.h");
        type QSize = cxx_qt_lib::QSize;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "QsScreencopyCapture"]
        #[qproperty(QString, output)]
        #[qproperty(bool, paint_cursor, cxx_name = "paintCursor")]
        #[qproperty(bool, live)]
        #[qproperty(bool, has_content, cxx_name = "hasContent", READ, NOTIFY)]
        #[qproperty(QImage, frame, READ, NOTIFY)]
        #[qproperty(QSize, buffer_size, cxx_name = "bufferSize", READ, NOTIFY)]
        #[qproperty(i32, buffer_transform, cxx_name = "bufferTransform", READ, NOTIFY)]
        type ScreencopyCapture = super::ScreencopyCaptureRust;

        /// Asks for one frame; false when there is no capture context (no screen, or the
        /// compositor lacks wlr-screencopy).
        #[qinvokable]
        #[cxx_name = "captureFrame"]
        fn capture_frame(self: Pin<&mut ScreencopyCapture>) -> bool;

        /// Mean Rec. 709 luma of the current frame over every pixel, 0 to 1 (ImageMagick's
        /// `-colorspace Gray` mean); -1 without a frame. qs-rs only (feature
        /// "screencopy-luminance"): inir's anti-flashbang read it from grim | magick.
        #[qinvokable]
        #[cxx_name = "meanLuminance"]
        fn mean_luminance(self: &ScreencopyCapture) -> f64;

        #[qsignal]
        fn stopped(self: Pin<&mut ScreencopyCapture>);
    }

    impl cxx_qt::Initialize for ScreencopyCapture {}
    impl cxx_qt::Threading for ScreencopyCapture {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QImage, QImageFormat, QSize, QString};
use qs_wayland::wayland_client::globals::{registry_queue_init, GlobalListContents};
use qs_wayland::wayland_client::protocol::{wl_buffer::WlBuffer, wl_output, wl_output::WlOutput, wl_registry, wl_shm, wl_shm::WlShm, wl_shm_pool::WlShmPool};
use qs_wayland::wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
use qs_wayland::wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1};
use qs_wayland::wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type Qt = cxx_qt::CxxQtThread<qobject::ScreencopyCapture>;

const LOG: &str = "quickshell.wayland.screencopy";

pub struct ScreencopyCaptureRust {
    output: QString,
    paint_cursor: bool,
    live: bool,
    has_content: bool,
    frame: QImage,
    buffer_size: QSize,
    buffer_transform: i32,
    current: Option<Arc<ShmBuffer>>,
    context: Option<Arc<Context>>,
    generation: u64,
    update_queued: bool,
}

impl Default for ScreencopyCaptureRust {
    fn default() -> Self {
        Self {
            output: QString::default(),
            paint_cursor: false,
            live: false,
            has_content: false,
            frame: QImage::default(),
            buffer_size: QSize::new(0, 0),
            buffer_transform: 0,
            current: None,
            context: None,
            generation: 0,
            update_queued: false,
        }
    }
}

/// One shared-memory buffer the compositor copies a frame into.
struct ShmBuffer {
    buffer: WlBuffer,
    data: *mut u8,
    len: usize,
    width: i32,
    height: i32,
    stride: i32,
    format: wl_shm::Format,
}

// The mapping is only written by the compositor while a copy is pending, and only read through
// QImages after `ready`; the wl_buffer proxy is thread safe.
unsafe impl Send for ShmBuffer {}
unsafe impl Sync for ShmBuffer {}

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        self.buffer.destroy();
        unsafe { libc::munmap(self.data.cast(), self.len) };
    }
}

impl ShmBuffer {
    fn new(g: &Globals, width: i32, height: i32, stride: i32, format: wl_shm::Format) -> Option<Self> {
        let len = (stride as usize).checked_mul(height as usize)?;
        let fd = unsafe { libc::memfd_create(c"qs-screencopy".as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return None;
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        if unsafe { libc::ftruncate(fd.as_raw_fd(), len as libc::off_t) } != 0 {
            return None;
        }
        let data = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd.as_raw_fd(), 0) };
        if data == libc::MAP_FAILED {
            return None;
        }
        // libwayland dups the fd when it marshals the request; the buffer outlives the pool.
        let pool: WlShmPool = g.shm.create_pool(fd.as_fd(), len as i32, &g.qh, ());
        let buffer = pool.create_buffer(0, width, height, stride, format, &g.qh, ());
        pool.destroy();
        Some(Self { buffer, data: data.cast(), len, width, height, stride, format })
    }

    /// Mean Rec. 709 luma (ImageMagick's weights) over every pixel, 0 to 1.
    fn mean_luma(&self) -> f64 {
        use wl_shm::Format as F;
        // Weights scaled to 16 bits; channel positions in a little endian 32-bit pixel.
        const W: [u64; 3] = [13937, 46868, 4731];
        let (r, g, b, ten) = match self.format {
            F::Xrgb8888 | F::Argb8888 => (16, 8, 0, false),
            F::Xbgr8888 | F::Abgr8888 => (0, 8, 16, false),
            F::Xrgb2101010 | F::Argb2101010 => (20, 10, 0, true),
            F::Xbgr2101010 | F::Abgr2101010 => (0, 10, 20, true),
            _ => return -1.0,
        };
        let (mask, max) = if ten { (0x3ff_u32, 1023.0) } else { (0xff_u32, 255.0) };
        let mut sum = 0u64;
        for row in 0..self.height as usize {
            let line = unsafe { std::slice::from_raw_parts(self.data.add(row * self.stride as usize), self.width as usize * 4) };
            let mut acc = 0u64;
            for px in line.chunks_exact(4) {
                let v = u32::from_le_bytes([px[0], px[1], px[2], px[3]]);
                acc += W[0] * ((v >> r) & mask) as u64 + W[1] * ((v >> g) & mask) as u64 + W[2] * ((v >> b) & mask) as u64;
            }
            sum += acc;
        }
        let pixels = self.width as f64 * self.height as f64;
        if pixels == 0.0 {
            return -1.0;
        }
        sum as f64 / 65536.0 / max / pixels
    }

    /// The QImage format that reads this buffer's bytes as they are (little endian).
    fn image_format(&self) -> Option<QImageFormat> {
        image_format(self.format)
    }
}

fn image_format(format: wl_shm::Format) -> Option<QImageFormat> {
    use wl_shm::Format as F;
    Some(match format {
        F::Xrgb8888 => QImageFormat::Format_RGB32,
        F::Argb8888 => QImageFormat::Format_ARGB32_Premultiplied,
        F::Xbgr8888 => QImageFormat::Format_RGBX8888,
        F::Abgr8888 => QImageFormat::Format_RGBA8888_Premultiplied,
        F::Xrgb2101010 => QImageFormat::Format_RGB30,
        F::Argb2101010 => QImageFormat::Format_A2RGB30_Premultiplied,
        F::Xbgr2101010 => QImageFormat::Format_BGR30,
        F::Abgr2101010 => QImageFormat::Format_A2BGR30_Premultiplied,
        _ => return None,
    })
}

/// Preference among the shm formats a frame offers: 8-bit first (what QImage and the scene
/// graph upload without conversion), then 10-bit.
fn format_rank(format: wl_shm::Format) -> Option<u8> {
    use wl_shm::Format as F;
    Some(match format {
        F::Xrgb8888 => 0,
        F::Argb8888 => 1,
        F::Xbgr8888 => 2,
        F::Abgr8888 => 3,
        F::Xrgb2101010 => 4,
        F::Xbgr2101010 => 5,
        F::Argb2101010 => 6,
        F::Abgr2101010 => 7,
        _ => return None,
    })
}

/// A capture of one screen with one cursor setting. Replaced (and shut down) whenever either
/// changes.
struct Context {
    qt: Qt,
    generation: u64,
    output: WlOutput,
    paint_cursor: bool,
    dead: AtomicBool,
    in_flight: Mutex<Option<ZwlrScreencopyFrameV1>>,
    copied_first: AtomicBool,
    pool: Mutex<Vec<Arc<ShmBuffer>>>,
}

impl Context {
    fn shutdown(&self) {
        self.dead.store(true, Ordering::Release);
        if let Some(frame) = self.in_flight.lock().unwrap().take() {
            frame.destroy();
        }
        self.pool.lock().unwrap().clear();
    }

    /// A buffer no QImage still reads, matching the frame's request; allocated when none is.
    fn backbuffer(&self, g: &Globals, width: i32, height: i32, stride: i32, format: wl_shm::Format) -> Option<Arc<ShmBuffer>> {
        let mut pool = self.pool.lock().unwrap();
        pool.retain(|b| Arc::strong_count(b) > 1 || (b.width == width && b.height == height && b.stride == stride && b.format == format));
        if let Some(b) = pool.iter().find(|b| Arc::strong_count(b) == 1 && b.width == width && b.height == height && b.stride == stride && b.format == format) {
            return Some(b.clone());
        }
        let b = Arc::new(ShmBuffer::new(g, width, height, stride, format)?);
        pool.push(b.clone());
        Some(b)
    }
}

#[derive(Default)]
struct FrameState {
    offers: Vec<(wl_shm::Format, u32, u32, u32)>,
    y_invert: bool,
    buffer: Option<Arc<ShmBuffer>>,
    copying: bool,
}

struct Frame {
    ctx: Arc<Context>,
    damage: bool,
    state: Mutex<FrameState>,
}

struct State;

struct Globals {
    conn: Connection,
    manager: ZwlrScreencopyManagerV1,
    shm: WlShm,
    qh: QueueHandle<State>,
}

/// Each screen's wl_output transform, from our own wl_output per global: Qt keeps only an
/// orientation (a pending transform it resets once applied), which loses flips, and niri's
/// winit output is Flipped180. Matched to screens by the version 4 name event.
struct OutputInfo {
    name: Mutex<String>,
    transform: AtomicI32,
    pending: AtomicI32,
}

fn outputs() -> &'static Mutex<Vec<(u32, WlOutput, Arc<OutputInfo>)>> {
    static O: OnceLock<Mutex<Vec<(u32, WlOutput, Arc<OutputInfo>)>>> = OnceLock::new();
    O.get_or_init(|| Mutex::new(Vec::new()))
}

fn bind_output(registry: &wl_registry::WlRegistry, name: u32, version: u32, qh: &QueueHandle<State>) {
    if version < 4 {
        return;
    }
    let info = Arc::new(OutputInfo { name: Mutex::new(String::new()), transform: AtomicI32::new(0), pending: AtomicI32::new(0) });
    let output = registry.bind::<WlOutput, _, _>(name, 4, qh, info.clone());
    outputs().lock().unwrap().push((name, output, info));
}

/// The wl_output transform of the screen named `name` (0 when unknown).
fn output_transform(name: &str) -> i32 {
    outputs().lock().unwrap().iter().find(|o| *o.2.name.lock().unwrap() == name).map_or(0, |o| o.2.transform.load(Ordering::Acquire))
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, registry: &wl_registry::WlRegistry, event: wl_registry::Event, _: &GlobalListContents, _: &Connection, qh: &QueueHandle<Self>) {
        match event {
            wl_registry::Event::Global { name, interface, version } if interface == WlOutput::interface().name => bind_output(registry, name, version, qh),
            wl_registry::Event::GlobalRemove { name } => {
                let mut list = outputs().lock().unwrap();
                if let Some(i) = list.iter().position(|o| o.0 == name) {
                    list.remove(i).1.release();
                }
            }
            _ => {}
        }
    }
}
impl Dispatch<WlOutput, Arc<OutputInfo>> for State {
    fn event(_: &mut Self, _: &WlOutput, event: wl_output::Event, info: &Arc<OutputInfo>, _: &Connection, _: &QueueHandle<Self>) {
        match event {
            wl_output::Event::Geometry { transform: WEnum::Value(t), .. } => info.pending.store(t as i32, Ordering::Release),
            wl_output::Event::Name { name } => *info.name.lock().unwrap() = name,
            wl_output::Event::Done => info.transform.store(info.pending.load(Ordering::Acquire), Ordering::Release),
            _ => {}
        }
    }
}
impl Dispatch<ZwlrScreencopyManagerV1, ()> for State {
    fn event(_: &mut Self, _: &ZwlrScreencopyManagerV1, _: <ZwlrScreencopyManagerV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<WlShm, ()> for State {
    fn event(_: &mut Self, _: &WlShm, _: wl_shm::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<WlShmPool, ()> for State {
    fn event(_: &mut Self, _: &WlShmPool, _: <WlShmPool as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<WlBuffer, ()> for State {
    fn event(_: &mut Self, _: &WlBuffer, _: <WlBuffer as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZwlrScreencopyFrameV1, Arc<Frame>> for State {
    fn event(_: &mut Self, proxy: &ZwlrScreencopyFrameV1, event: zwlr_screencopy_frame_v1::Event, data: &Arc<Frame>, _: &Connection, _: &QueueHandle<Self>) {
        use zwlr_screencopy_frame_v1::Event as E;
        let ctx = &data.ctx;
        if ctx.dead.load(Ordering::Acquire) {
            return;
        }
        match event {
            E::Buffer { format: WEnum::Value(format), width, height, stride } => {
                data.state.lock().unwrap().offers.push((format, width, height, stride));
                // Version 1 has no buffer_done: the shm offer is the whole request.
                if proxy.version() < 3 {
                    start_copy(proxy, data);
                }
            }
            E::Flags { flags: WEnum::Value(flags) } => {
                data.state.lock().unwrap().y_invert = flags.contains(zwlr_screencopy_frame_v1::Flags::YInvert);
            }
            E::BufferDone => start_copy(proxy, data),
            E::Ready { .. } => {
                let (buffer, y_invert) = {
                    let mut st = data.state.lock().unwrap();
                    (st.buffer.take(), st.y_invert)
                };
                ctx.copied_first.store(true, Ordering::Release);
                if let Some(frame) = ctx.in_flight.lock().unwrap().take() {
                    frame.destroy();
                }
                let Some(buffer) = buffer else { return };
                let generation = ctx.generation;
                let _ = ctx.qt.queue(move |this: Pin<&mut qobject::ScreencopyCapture>| this.present(generation, buffer, y_invert));
            }
            E::Failed => {
                if let Some(frame) = ctx.in_flight.lock().unwrap().take() {
                    frame.destroy();
                }
                quickshell::qs_log!(warn, LOG, "Ending recording due to screencopy failure.");
                let generation = ctx.generation;
                let _ = ctx.qt.queue(move |this: Pin<&mut qobject::ScreencopyCapture>| this.fail(generation));
            }
            _ => {}
        }
    }
}

/// buffer_done: pick the best shm offer, take a free buffer for it and ask for the copy.
fn start_copy(proxy: &ZwlrScreencopyFrameV1, frame: &Arc<Frame>) {
    let Some(g) = globals() else { return };
    let mut st = frame.state.lock().unwrap();
    if st.copying {
        return;
    }
    let Some(&(format, width, height, stride)) = st.offers.iter().filter(|o| format_rank(o.0).is_some()).min_by_key(|o| format_rank(o.0)) else {
        drop(st);
        quickshell::qs_log!(warn, LOG, "Compositor offered no shm format qs-rs can read; stopping the capture.");
        let generation = frame.ctx.generation;
        let _ = frame.ctx.qt.queue(move |this: Pin<&mut qobject::ScreencopyCapture>| this.fail(generation));
        return;
    };
    let Some(buffer) = frame.ctx.backbuffer(g, width as i32, height as i32, stride as i32, format) else {
        drop(st);
        quickshell::qs_log!(warn, LOG, "Backbuffer creation failed for screencopy. Skipping frame.");
        let generation = frame.ctx.generation;
        let _ = frame.ctx.qt.queue(move |this: Pin<&mut qobject::ScreencopyCapture>| this.fail(generation));
        return;
    };
    st.copying = true;
    // copy_with_damage waits for the screen to change: right for a live view (the compositor
    // paces it), wrong for a one-shot capture of a still screen, which would never return.
    if frame.damage && proxy.version() >= 2 {
        proxy.copy_with_damage(&buffer.buffer);
    } else {
        proxy.copy(&buffer.buffer);
    }
    st.buffer = Some(buffer);
}

/// The manager and wl_shm on a dedicated queue; `None` (with a warning, once) when the
/// compositor lacks wlr-screencopy or Qt is not on Wayland. Qt thread only on first call.
fn globals() -> Option<&'static Globals> {
    static G: OnceLock<Option<Globals>> = OnceLock::new();
    G.get_or_init(|| {
        let conn = qs_wayland::connection()?;
        let (list, queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();
        let manager = match list.bind::<ZwlrScreencopyManagerV1, _, _>(&qh, 1..=3, ()) {
            Ok(m) => m,
            Err(_) => {
                quickshell::qs_log!(warn, LOG, "Cannot capture screens as wlr-screencopy-unstable-v1 is not supported by the current compositor.");
                return None;
            }
        };
        let shm = list.bind::<WlShm, _, _>(&qh, 1..=1, ()).ok()?;
        for g in list.contents().clone_list() {
            if g.interface == WlOutput::interface().name {
                bind_output(list.registry(), g.name, g.version, &qh);
            }
        }
        qs_wayland::spawn_dispatch("wayland-screencopy", queue, State);
        Some(Globals { conn, manager, shm, qh })
    })
    .as_ref()
}

extern "C" fn release_buffer(info: *mut cxx_qt_lib::c_void) {
    // Safety: `info` is the Box<Arc<ShmBuffer>> leaked in `present`, released once by QImage.
    drop(unsafe { Box::from_raw(info.cast::<Arc<ShmBuffer>>()) });
}

impl cxx_qt::Initialize for qobject::ScreencopyCapture {
    fn initialize(mut self: Pin<&mut Self>) {
        // The initial QML assignments coalesce into one context.
        self.as_mut().on_output_changed(|this| this.queue_update()).release();
        self.as_mut().on_paint_cursor_changed(|this| this.queue_update()).release();
        self.as_mut()
            .on_live_changed(|this| {
                if this.live {
                    this.capture_frame();
                }
            })
            .release();
        self.queue_update();
    }
}

impl Drop for ScreencopyCaptureRust {
    fn drop(&mut self) {
        if let Some(ctx) = self.context.take() {
            ctx.shutdown();
        }
    }
}

impl qobject::ScreencopyCapture {
    fn queue_update(mut self: Pin<&mut Self>) {
        if self.update_queued {
            return;
        }
        self.as_mut().rust_mut().update_queued = true;
        let _ = self.qt_thread().queue(|this| this.update());
    }

    fn update(mut self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.update_queued = false;
            rust.generation += 1;
            if let Some(ctx) = rust.context.take() {
                ctx.shutdown();
            }
        }
        self.as_mut().clear();
        let name = self.output.to_string();
        if name.is_empty() {
            return;
        }
        if globals().is_none() {
            return;
        }
        let Some(output) = qs_wayland::output_for_screen_name(&name) else {
            quickshell::qs_log!(warn, LOG, "No wayland output for screen {name}; nothing to capture.");
            return;
        };
        let ctx = Arc::new(Context {
            qt,
            generation: self.generation,
            output,
            paint_cursor: self.paint_cursor,
            dead: AtomicBool::new(false),
            in_flight: Mutex::new(None),
            copied_first: AtomicBool::new(false),
            pool: Mutex::new(Vec::new()),
        });
        self.as_mut().rust_mut().context = Some(ctx);
        self.capture_frame();
    }

    fn capture_frame(self: Pin<&mut Self>) -> bool {
        let Some(ctx) = self.context.clone() else { return false };
        let Some(g) = globals() else { return false };
        let mut in_flight = ctx.in_flight.lock().unwrap();
        if in_flight.is_some() {
            return true;
        }
        let damage = self.live && ctx.copied_first.load(Ordering::Acquire);
        let frame = Arc::new(Frame { ctx: ctx.clone(), damage, state: Mutex::new(FrameState::default()) });
        *in_flight = Some(g.manager.capture_output(ctx.paint_cursor as i32, &ctx.output, &g.qh, frame));
        drop(in_flight);
        let _ = g.conn.flush();
        true
    }

    fn present(mut self: Pin<&mut Self>, generation: u64, buffer: Arc<ShmBuffer>, y_invert: bool) {
        if generation != self.generation {
            return;
        }
        let Some(format) = buffer.image_format() else { return };
        let (width, height, stride) = (buffer.width, buffer.height, buffer.stride);
        self.as_mut().rust_mut().current = Some(buffer.clone());
        let image = if stride == width * 4 {
            let data = buffer.data as *const u8;
            let info = Box::into_raw(Box::new(buffer)).cast();
            // Safety: the mapping is width*height*4 bytes in `format`, kept alive by the Arc
            // released through `release_buffer` when the last QImage copy goes.
            unsafe { QImage::from_raw_parts(data, width, height, format, release_buffer, info) }
        } else {
            // Padded rows (QImage without bytesPerLine assumes none): repack once.
            let mut bytes = Vec::with_capacity((width * height * 4) as usize);
            for row in 0..height as usize {
                let start = row * stride as usize;
                bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(buffer.data.add(start), width as usize * 4) });
            }
            unsafe { QImage::from_raw_bytes(bytes, width, height, format) }
        };
        // Upstream: the output's transform, flipped 180 (a vertical mirror) when y-inverted.
        let transform = output_transform(&self.output.to_string()) ^ if y_invert { 6 } else { 0 };
        self.as_mut().set_frame(image);
        if self.buffer_transform != transform {
            self.as_mut().rust_mut().buffer_transform = transform;
            self.as_mut().buffer_transform_changed();
        }
        let size = QSize::new(width, height);
        if self.buffer_size != size {
            self.as_mut().rust_mut().buffer_size = size;
            self.as_mut().buffer_size_changed();
        }
        if !self.has_content {
            self.as_mut().rust_mut().has_content = true;
            self.as_mut().has_content_changed();
        }
        if self.live {
            self.capture_frame();
        }
    }

    fn fail(mut self: Pin<&mut Self>, generation: u64) {
        if generation != self.generation {
            return;
        }
        if let Some(ctx) = self.as_mut().rust_mut().context.take() {
            ctx.shutdown();
        }
        self.as_mut().clear();
        self.stopped();
    }

    fn mean_luminance(&self) -> f64 {
        self.current.as_ref().map_or(-1.0, |b| b.mean_luma())
    }

    fn set_frame(mut self: Pin<&mut Self>, image: QImage) {
        self.as_mut().rust_mut().frame = image;
        self.frame_changed();
    }

    fn clear(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().current = None;
        if self.has_content {
            self.as_mut().rust_mut().has_content = false;
            self.as_mut().has_content_changed();
        }
        if !self.frame.is_null() {
            self.as_mut().set_frame(QImage::default());
        }
        if self.buffer_size != QSize::new(0, 0) {
            self.as_mut().rust_mut().buffer_size = QSize::new(0, 0);
            self.as_mut().buffer_size_changed();
        }
    }
}
