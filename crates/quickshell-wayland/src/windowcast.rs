//! ScreencopyView capture of single windows (upstream's toplevel capture). niri 26.04 has neither
//! hyprland-toplevel-export nor ext-image-copy-capture, but it exports Mutter's ScreenCast D-Bus
//! API without a dialog: RecordWindow takes a niri window id and the window arrives as a PipeWire
//! stream of DMA-BUFs. The D-Bus half runs on the qs-dbus thread, the stream on one shared
//! PipeWire thread, and QsWindowCastItem (windowcast.h) imports each buffer as an EGLImage
//! texture on the render thread: no copy through the CPU.
//!
//! Buffers are held, not copied: the newest dequeued buffer waits in `Frames::pending` until the
//! item takes it, and the item hands it back once a newer one replaced it on screen (niri keeps
//! up to 16, and only queues a buffer after its rendering finished).

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qsize.h");
        type QSize = cxx_qt_lib::QSize;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "QsWindowCapture"]
        /// niri window id to record; -1 resolves `appId`/`title` instead.
        #[qproperty(i32, window_id, cxx_name = "windowId")]
        #[qproperty(QString, app_id, cxx_name = "appId")]
        #[qproperty(QString, title)]
        #[qproperty(bool, paint_cursor, cxx_name = "paintCursor")]
        #[qproperty(bool, live)]
        #[qproperty(bool, has_content, cxx_name = "hasContent", READ, NOTIFY)]
        #[qproperty(QSize, buffer_size, cxx_name = "bufferSize", READ, NOTIFY)]
        /// The frame queue QsWindowCastItem reads (an `Arc<Frames>` pointer, fixed for life).
        #[qproperty(u64, frames, READ, CONSTANT)]
        type WindowCapture = super::WindowCaptureRust;

        /// Asks for one more frame (live: false); false without a recording.
        #[qinvokable]
        #[cxx_name = "captureFrame"]
        fn capture_frame(self: Pin<&mut WindowCapture>) -> bool;

        /// A frame waits in the queue: the item should repaint.
        #[qsignal]
        #[cxx_name = "frameReady"]
        fn frame_ready(self: Pin<&mut WindowCapture>);

        #[qsignal]
        fn stopped(self: Pin<&mut WindowCapture>);
    }

    impl cxx_qt::Initialize for WindowCapture {}
    impl cxx_qt::Threading for WindowCapture {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QSize, QString};
use pipewire as pw;
use pw::spa;
use crate::castffi::ffi::{self as castffi, CastFrame};
use qs_dbus::zbus;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type Qt = cxx_qt::CxxQtThread<qobject::WindowCapture>;

const LOG: &str = "quickshell.wayland.windowcast";
const SCREENCAST: &str = "org.gnome.Mutter.ScreenCast";
const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
const DRM_FORMAT_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

pub struct WindowCaptureRust {
    window_id: i32,
    app_id: QString,
    title: QString,
    paint_cursor: bool,
    live: bool,
    has_content: bool,
    buffer_size: QSize,
    frames: u64,
    shared: Arc<Frames>,
    session: Option<Arc<Session>>,
    update_queued: bool,
}

impl Default for WindowCaptureRust {
    fn default() -> Self {
        let shared = Arc::new(Frames::default());
        Self {
            window_id: -1,
            app_id: QString::default(),
            title: QString::default(),
            paint_cursor: false,
            live: false,
            has_content: false,
            buffer_size: QSize::new(0, 0),
            frames: Arc::as_ptr(&shared) as u64,
            shared,
            session: None,
            update_queued: false,
        }
    }
}

/// What the stream and the render thread share: the newest frame and the buffers behind it.
#[derive(Default)]
pub struct Frames {
    state: Mutex<FramesState>,
    /// Set while a frameReady is queued on the Qt thread (one per repaint, not per buffer).
    notify_queued: AtomicBool,
    qt: Mutex<Option<Qt>>,
}

#[derive(Default)]
struct FramesState {
    format: Option<Format>,
    buffers: HashMap<u64, Planes>,
    pending: Option<u64>,
}

#[derive(Clone, Copy, PartialEq)]
struct Format {
    width: u32,
    height: u32,
    fourcc: u32,
    modifier: u64,
}

#[derive(Clone, Copy, Default)]
struct Planes {
    count: u32,
    fds: [i32; 4],
    offsets: [u32; 4],
    strides: [u32; 4],
}

impl Frames {
    fn notify(&self, f: impl FnOnce(Pin<&mut qobject::WindowCapture>) + Send + 'static) {
        if let Some(qt) = self.qt.lock().unwrap().as_ref() {
            let _ = qt.queue(f);
        }
    }

    fn frame_arrived(&self) {
        if self.notify_queued.swap(true, Ordering::AcqRel) {
            return;
        }
        let this = self as *const Frames as usize;
        self.notify(move |mut obj| {
            // The object owns an Arc to this Frames, so it is alive while the object is.
            if Arc::as_ptr(&obj.shared) as usize == this {
                obj.shared.notify_queued.store(false, Ordering::Release);
                if !obj.has_content {
                    obj.as_mut().rust_mut().has_content = true;
                    obj.as_mut().has_content_changed();
                }
                obj.frame_ready();
            }
        });
    }
}

/// Stream ids live in the top bits of every buffer key, so keys never repeat across streams.
const KEY_SHIFT: u32 = 24;

unsafe fn frames_ref<'a>(frames: usize) -> Option<&'a Frames> {
    // Safety (caller): `frames` is the `frames` property of a live QsWindowCapture or a pointer
    // kept alive with cast_retain.
    (frames != 0).then(|| unsafe { &*(frames as *const Frames) })
}

pub unsafe fn cast_take(frames: usize, out: &mut CastFrame) -> bool {
    let Some(frames) = (unsafe { frames_ref(frames) }) else { return false };
    let mut state = frames.state.lock().unwrap();
    let (Some(key), Some(format)) = (state.pending.take(), state.format) else { return false };
    let Some(planes) = state.buffers.get(&key).copied() else { return false };
    out.key = key;
    out.width = format.width;
    out.height = format.height;
    out.fourcc = format.fourcc;
    out.modifier = format.modifier;
    out.planes = planes.count;
    for i in 0..planes.count as usize {
        // Duplicated under the lock: PipeWire closes its fds after remove_buffer, which takes it.
        out.fds[i] = unsafe { libc::fcntl(planes.fds[i], libc::F_DUPFD_CLOEXEC, 0) };
        out.offsets[i] = planes.offsets[i];
        out.strides[i] = planes.strides[i];
    }
    true
}

pub unsafe fn cast_release(_frames: usize, key: u64) {
    send(Cmd::Release(key >> KEY_SHIFT, key));
}

pub unsafe fn cast_alive(frames: usize, key: u64) -> bool {
    let Some(frames) = (unsafe { frames_ref(frames) }) else { return false };
    frames.state.lock().unwrap().buffers.contains_key(&key)
}

pub unsafe fn cast_retain(frames: usize) {
    if frames != 0 {
        unsafe { Arc::increment_strong_count(frames as *const Frames) };
    }
}

pub unsafe fn cast_unref(frames: usize) {
    if frames != 0 {
        unsafe { Arc::decrement_strong_count(frames as *const Frames) };
    }
}

// --- PipeWire thread ---

enum Cmd {
    Open { id: u64, node: u32, frames: Arc<Frames>, live: Arc<AtomicBool> },
    Close(u64),
    Release(u64, u64),
    Resume(u64),
}

fn send(cmd: Cmd) {
    static TX: OnceLock<Mutex<pw::channel::Sender<Cmd>>> = OnceLock::new();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = pw::channel::channel::<Cmd>();
        std::thread::Builder::new()
            .name("windowcast".into())
            .spawn(move || {
                if let Err(e) = run(rx) {
                    quickshell::qs_log!(warn, LOG, "PipeWire: {e}");
                }
            })
            .expect("spawn windowcast thread");
        Mutex::new(tx)
    });
    let _ = tx.lock().unwrap().send(cmd);
}

struct Entry {
    stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<StreamData>,
    held: std::rc::Rc<std::cell::RefCell<HashMap<u64, *mut pw::sys::pw_buffer>>>,
    frames: Arc<Frames>,
}

struct StreamData {
    id: u64,
    seq: u64,
    frames: Arc<Frames>,
    live: Arc<AtomicBool>,
    held: std::rc::Rc<std::cell::RefCell<HashMap<u64, *mut pw::sys::pw_buffer>>>,
}

fn run(rx: pw::channel::Receiver<Cmd>) -> Result<(), pw::Error> {
    pw::init();
    let main_loop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&main_loop, None)?;
    let core = context.connect_rc(None)?;
    let entries = std::rc::Rc::new(std::cell::RefCell::new(HashMap::<u64, Entry>::new()));
    let _rx = rx.attach(main_loop.loop_(), {
        let entries = entries.clone();
        move |cmd| match cmd {
            Cmd::Open { id, node, frames, live } => match open(&core, id, node, frames, live) {
                Ok(entry) => {
                    entries.borrow_mut().insert(id, entry);
                }
                Err(e) => quickshell::qs_log!(warn, LOG, "Cannot open the window stream: {e}"),
            },
            Cmd::Close(id) => {
                if let Some(entry) = entries.borrow_mut().remove(&id) {
                    let _ = entry.stream.disconnect();
                    let mut state = entry.frames.state.lock().unwrap();
                    state.buffers.retain(|key, _| key >> KEY_SHIFT != id);
                    if state.pending.is_some_and(|key| key >> KEY_SHIFT == id) {
                        state.pending = None;
                    }
                }
            }
            Cmd::Release(id, key) => {
                if let Some(entry) = entries.borrow().get(&id) {
                    if let Some(buffer) = entry.held.borrow_mut().remove(&key) {
                        unsafe { entry.stream.queue_raw_buffer(buffer) };
                    }
                }
            }
            Cmd::Resume(id) => {
                if let Some(entry) = entries.borrow().get(&id) {
                    let _ = entry.stream.set_active(true);
                }
            }
        }
    });
    main_loop.run();
    Ok(())
}

fn serialize(value: spa::pod::Value) -> Vec<u8> {
    spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &value).unwrap().0.into_inner()
}

/// One EnumFormat per pixel format: niri sends BGRA for windows with alpha, BGRx otherwise, and
/// fixates the modifier from our list (DONT_FIXATE) after a test allocation.
fn enum_format(format: spa::param::video::VideoFormat, fourcc: u32) -> Option<Vec<u8>> {
    use spa::param::format::{FormatProperties, MediaSubtype, MediaType};
    use spa::pod::{ChoiceValue, Property, PropertyFlags, Value};
    use spa::utils::{Choice, ChoiceEnum, ChoiceFlags, Id};
    let modifiers: Vec<i64> = castffi::dmabuf_modifiers(fourcc).into_iter().map(|m| m as i64).collect();
    let first = *modifiers.first()?;
    let id = |key: FormatProperties, value: u32| Property::new(key.as_raw(), Value::Id(Id(value)));
    let object = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: vec![
            id(FormatProperties::MediaType, MediaType::Video.as_raw()),
            id(FormatProperties::MediaSubtype, MediaSubtype::Raw.as_raw()),
            id(FormatProperties::VideoFormat, format.as_raw()),
            Property {
                key: FormatProperties::VideoModifier.as_raw(),
                // SPA_POD_PROP_FLAG_DONT_FIXATE (1 << 4); libspa names it only behind its v0_3_33 feature.
                flags: PropertyFlags::MANDATORY | PropertyFlags::from_bits_retain(1 << 4),
                value: Value::Choice(ChoiceValue::Long(Choice(ChoiceFlags::empty(), ChoiceEnum::Enum { default: first, alternatives: modifiers }))),
            },
            Property::new(
                FormatProperties::VideoSize.as_raw(),
                Value::Choice(ChoiceValue::Rectangle(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Range {
                        default: spa::utils::Rectangle { width: 640, height: 480 },
                        min: spa::utils::Rectangle { width: 1, height: 1 },
                        max: spa::utils::Rectangle { width: 16384, height: 16384 },
                    },
                ))),
            ),
        ],
    };
    Some(serialize(Value::Object(object)))
}

/// DMA-BUF buffers only, plus the header meta (its flags mark corrupted frames).
fn buffer_params() -> [Vec<u8>; 2] {
    use spa::pod::{Property, Value};
    let sys = spa::sys::SPA_DATA_DmaBuf;
    let buffers = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamBuffers.as_raw(),
        id: spa::param::ParamType::Buffers.as_raw(),
        properties: vec![Property::new(spa::sys::SPA_PARAM_BUFFERS_dataType, Value::Int(1 << sys))],
    };
    let meta = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamMeta.as_raw(),
        id: spa::param::ParamType::Meta.as_raw(),
        properties: vec![
            Property::new(spa::sys::SPA_PARAM_META_type, Value::Id(spa::utils::Id(spa::sys::SPA_META_Header))),
            Property::new(spa::sys::SPA_PARAM_META_size, Value::Int(std::mem::size_of::<spa::sys::spa_meta_header>() as i32)),
        ],
    };
    [serialize(Value::Object(buffers)), serialize(Value::Object(meta))]
}

/// The negotiated format, or None while the modifier is still a choice (niri fixates it first).
/// Fixed values come wrapped as single-value choices (`ChoiceEnum::None`).
fn parse_format(param: &spa::pod::Pod) -> Option<Format> {
    use spa::param::format::FormatProperties;
    use spa::param::video::VideoFormat;
    use spa::pod::{ChoiceValue, Value};
    use spa::utils::{Choice, ChoiceEnum};
    let (_, Value::Object(object)) = spa::pod::deserialize::PodDeserializer::deserialize_any_from(param.as_bytes()).ok()? else {
        return None;
    };
    let prop = |key: FormatProperties| object.properties.iter().find(|p| p.key == key.as_raw()).map(|p| &p.value);
    let fourcc = match prop(FormatProperties::VideoFormat)? {
        Value::Id(id) | Value::Choice(ChoiceValue::Id(Choice(_, ChoiceEnum::None(id)))) => match id.0 {
            f if f == VideoFormat::BGRA.as_raw() => DRM_FORMAT_ARGB8888,
            f if f == VideoFormat::BGRx.as_raw() => DRM_FORMAT_XRGB8888,
            _ => return None,
        },
        _ => return None,
    };
    let modifier = match prop(FormatProperties::VideoModifier) {
        Some(Value::Long(m) | Value::Choice(ChoiceValue::Long(Choice(_, ChoiceEnum::None(m))))) => *m as u64,
        None => DRM_FORMAT_MOD_INVALID,
        _ => return None,
    };
    let size = match prop(FormatProperties::VideoSize)? {
        Value::Rectangle(r) | Value::Choice(ChoiceValue::Rectangle(Choice(_, ChoiceEnum::None(r)))) => *r,
        _ => return None,
    };
    Some(Format { width: size.width, height: size.height, fourcc, modifier })
}

fn open(core: &pw::core::CoreRc, id: u64, node: u32, frames: Arc<Frames>, live: Arc<AtomicBool>) -> Result<Entry, pw::Error> {
    use pw::properties::properties;
    let stream = pw::stream::StreamRc::new(
        core.clone(),
        "qs-rs window preview",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )?;
    let held = std::rc::Rc::new(std::cell::RefCell::new(HashMap::new()));
    let data = StreamData { id, seq: 0, frames: frames.clone(), live, held: held.clone() };
    let listener = stream
        .add_local_listener_with_user_data(data)
        .state_changed(|_, data, _, new| {
            if let pw::stream::StreamState::Error(e) = &new {
                quickshell::qs_log!(warn, LOG, "Window stream failed: {e}");
            }
            if matches!(new, pw::stream::StreamState::Error(_) | pw::stream::StreamState::Unconnected) {
                // Only for the current session: closing a replaced one disconnects it too.
                let id = data.id;
                data.frames.notify(move |mut obj| {
                    if obj.session.as_ref().is_some_and(|s| s.id == id) {
                        obj.as_mut().stop_from_stream();
                    }
                });
            }
        })
        .param_changed(|stream, data, id, param| {
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Some(format) = param.and_then(parse_format) else { return };
            let changed = data.frames.state.lock().unwrap().format.replace(format) != Some(format);
            if changed {
                let (w, h, id) = (format.width as i32, format.height as i32, data.id);
                data.frames.notify(move |mut obj| {
                    let size = QSize::new(w, h);
                    if obj.session.as_ref().is_some_and(|s| s.id == id) && obj.buffer_size != size {
                        obj.as_mut().rust_mut().buffer_size = size;
                        obj.as_mut().buffer_size_changed();
                    }
                });
            }
            let params = buffer_params();
            let mut pods: Vec<&spa::pod::Pod> = params.iter().filter_map(|p| spa::pod::Pod::from_bytes(p)).collect();
            let _ = stream.update_params(&mut pods);
        })
        .add_buffer(|_, data, buffer| {
            data.seq += 1;
            let key = (data.id << KEY_SHIFT) | data.seq;
            unsafe { (*buffer).user_data = key as *mut _ };
        })
        .remove_buffer(|_, data, buffer| {
            let key = unsafe { (*buffer).user_data } as u64;
            data.held.borrow_mut().remove(&key);
            let mut state = data.frames.state.lock().unwrap();
            state.buffers.remove(&key);
            if state.pending == Some(key) {
                state.pending = None;
            }
        })
        .process(|stream, data| process(stream, data))
        .register()?;
    let formats: Vec<Vec<u8>> = [(spa::param::video::VideoFormat::BGRA, DRM_FORMAT_ARGB8888), (spa::param::video::VideoFormat::BGRx, DRM_FORMAT_XRGB8888)]
        .into_iter()
        .filter_map(|(f, fourcc)| enum_format(f, fourcc))
        .collect();
    if formats.is_empty() {
        quickshell::qs_log!(warn, LOG, "EGL imports no DMA-BUF modifiers; window capture needs them.");
        return Err(pw::Error::CreationFailed);
    }
    let mut pods: Vec<&spa::pod::Pod> = formats.iter().filter_map(|p| spa::pod::Pod::from_bytes(p)).collect();
    stream.connect(spa::utils::Direction::Input, Some(node), pw::stream::StreamFlags::AUTOCONNECT, &mut pods)?;
    Ok(Entry { stream, _listener: listener, held, frames })
}

fn process(stream: &pw::stream::Stream, data: &mut StreamData) {
    // Only the newest buffer matters; older ones go straight back.
    let mut newest: *mut pw::sys::pw_buffer = std::ptr::null_mut();
    loop {
        let buffer = unsafe { stream.dequeue_raw_buffer() };
        if buffer.is_null() {
            break;
        }
        if !newest.is_null() {
            unsafe { stream.queue_raw_buffer(newest) };
        }
        newest = buffer;
    }
    if newest.is_null() {
        return;
    }
    let key = unsafe { (*newest).user_data } as u64;
    let Some(planes) = (unsafe { planes_of(newest) }) else {
        unsafe { stream.queue_raw_buffer(newest) };
        return;
    };
    let previous = {
        let mut state = data.frames.state.lock().unwrap();
        state.buffers.insert(key, planes);
        state.pending.replace(key)
    };
    let mut held = data.held.borrow_mut();
    held.insert(key, newest);
    // A frame nobody took yet is replaced: back to niri.
    if let Some(old) = previous.filter(|old| *old != key) {
        if let Some(buffer) = held.remove(&old) {
            unsafe { stream.queue_raw_buffer(buffer) };
        }
    }
    drop(held);
    data.frames.frame_arrived();
    if !data.live.load(Ordering::Acquire) {
        let _ = stream.set_active(false);
    }
}

/// The DMA-BUF planes of a buffer, or None for a corrupted or empty one.
unsafe fn planes_of(buffer: *mut pw::sys::pw_buffer) -> Option<Planes> {
    let spa_buffer = unsafe { &*(*buffer).buffer };
    for i in 0..spa_buffer.n_metas as usize {
        let meta = unsafe { &*spa_buffer.metas.add(i) };
        if meta.type_ == spa::sys::SPA_META_Header && !meta.data.is_null() {
            let header = unsafe { &*(meta.data as *const spa::sys::spa_meta_header) };
            if header.flags & spa::sys::SPA_META_HEADER_FLAG_CORRUPTED != 0 {
                return None;
            }
        }
    }
    let count = (spa_buffer.n_datas as usize).min(4);
    if count == 0 {
        return None;
    }
    let mut planes = Planes { count: count as u32, ..Default::default() };
    for i in 0..count {
        let d = unsafe { &*spa_buffer.datas.add(i) };
        if d.type_ != spa::sys::SPA_DATA_DmaBuf || d.chunk.is_null() {
            return None;
        }
        let chunk = unsafe { &*d.chunk };
        if chunk.flags & spa::sys::SPA_CHUNK_FLAG_CORRUPTED as i32 != 0 {
            return None;
        }
        planes.fds[i] = d.fd as i32;
        planes.offsets[i] = chunk.offset;
        planes.strides[i] = chunk.stride as u32;
    }
    Some(planes)
}

// --- D-Bus half: one ScreenCast session per recording ---

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct Session {
    id: u64,
    dead: AtomicBool,
    path: Mutex<Option<zbus::zvariant::OwnedObjectPath>>,
    live: Arc<AtomicBool>,
}

enum Target {
    Id(u64),
    Match { app_id: String, title: String },
}

impl Session {
    fn shutdown(&self) {
        self.dead.store(true, Ordering::Release);
        send(Cmd::Close(self.id));
        if let Some(path) = self.path.lock().unwrap().take() {
            qs_dbus::rt::spawn(async move {
                if let Ok(conn) = qs_dbus::rt::session().await {
                    stop(&conn, path.as_str()).await;
                }
            });
        }
    }
}

async fn stop(conn: &zbus::Connection, path: &str) {
    let _ = qs_dbus::call(conn, SCREENCAST, path, "org.gnome.Mutter.ScreenCast.Session", "Stop", None).await;
    sessions::forget(path);
}

/// Stops every recording this process holds and waits for niri to confirm, at most `timeout`:
/// the shell's last step on a normal exit (SIGTERM included), so niri drops the sessions now
/// instead of on the next shell's first capture.
pub fn stop_all(timeout: std::time::Duration) {
    let open = sessions::open();
    if open.is_empty() {
        return;
    }
    let (done, wait) = std::sync::mpsc::channel();
    let count = open.len();
    qs_dbus::rt::spawn(async move {
        if let Ok(conn) = qs_dbus::rt::session().await {
            for path in &open {
                stop(&conn, path).await;
            }
        }
        let _ = done.send(());
    });
    match wait.recv_timeout(timeout) {
        Ok(()) => quickshell::qs_log!(debug, LOG, "Stopped {count} window recordings on exit."),
        Err(_) => quickshell::qs_log!(warn, LOG, "niri did not stop {count} window recordings in time; the next shell will."),
    }
}

/// niri 26.04 keeps a session when its D-Bus client dies (Mutter stops it), so a killed shell
/// would leave its recordings behind. Each process lists its sessions in
/// `$XDG_RUNTIME_DIR/qs-rs/windowcast-<pid>`; the first recording of a new process stops the ones
/// a dead process left.
mod sessions {
    use std::path::PathBuf;
    use std::sync::Mutex;

    static OPEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn dir() -> Option<PathBuf> {
        std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("qs-rs"))
    }

    fn write(open: &[String]) {
        let Some(dir) = dir() else { return };
        let file = dir.join(format!("windowcast-{}", std::process::id()));
        if open.is_empty() {
            let _ = std::fs::remove_file(file);
        } else if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(file, open.join("\n"));
        }
    }

    pub fn remember(path: &str) {
        let mut open = OPEN.lock().unwrap();
        open.push(path.to_owned());
        write(&open);
    }

    pub fn forget(path: &str) {
        let mut open = OPEN.lock().unwrap();
        open.retain(|p| p != path);
        write(&open);
    }

    /// This process's open sessions, still listed on disk until each one is stopped.
    pub fn open() -> Vec<String> {
        OPEN.lock().unwrap().clone()
    }

    /// Session paths left by processes that are gone; their lists are removed.
    pub fn orphans() -> Vec<String> {
        let Some(entries) = dir().and_then(|d| std::fs::read_dir(d).ok()) else { return vec![] };
        let mut out = vec![];
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(|n| n.strip_prefix("windowcast-")).and_then(|p| p.parse::<u32>().ok()) else { continue };
            if pid == std::process::id() || std::path::Path::new(&format!("/proc/{pid}")).exists() {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                out.extend(text.lines().filter(|l| !l.is_empty()).map(str::to_owned));
            }
            let _ = std::fs::remove_file(entry.path());
        }
        out
    }
}

/// The niri window for a Toplevel: org.gnome.Shell.Introspect lists niri's windows with app ids
/// ("kitty.desktop") and titles. wlr-foreign-toplevel carries no window id, so twin windows
/// (same app and title) resolve to the oldest; `windowId` is exact.
async fn resolve(conn: &zbus::Connection, app_id: &str, title: &str) -> zbus::Result<Option<u64>> {
    use zbus::zvariant::OwnedValue;
    let p = zbus::Proxy::new(conn, "org.gnome.Shell.Introspect", "/org/gnome/Shell/Introspect", "org.gnome.Shell.Introspect").await?;
    let windows: HashMap<u64, HashMap<String, OwnedValue>> = p.call("GetWindows", &()).await?;
    let text = |props: &HashMap<String, OwnedValue>, key: &str| props.get(key).and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default();
    let mut best: Option<(u8, u64)> = None;
    for (id, props) in &windows {
        let app = text(props, "app-id");
        if app.strip_suffix(".desktop").unwrap_or(&app) != app_id {
            continue;
        }
        let score = if text(props, "title") == title { 2 } else { 1 };
        if best.is_none_or(|(s, i)| score > s || (score == s && *id < i)) {
            best = Some((score, *id));
        }
    }
    Ok(best.map(|(_, id)| id))
}

async fn start(session: Arc<Session>, target: Target, cursor: u32, frames: Arc<Frames>) -> zbus::Result<()> {
    use zbus::zvariant::{OwnedObjectPath, Value};
    let conn = qs_dbus::rt::session().await?;
    let window = match target {
        Target::Id(id) => id,
        Target::Match { app_id, title } => match resolve(&conn, &app_id, &title).await? {
            Some(id) => id,
            None => {
                quickshell::qs_log!(info, LOG, "No niri window for {app_id}; nothing to capture.");
                return Ok(());
            }
        },
    };
    quickshell::qs_log!(debug, LOG, "Recording niri window {window}.");
    let sc = zbus::Proxy::new(&conn, SCREENCAST, "/org/gnome/Mutter/ScreenCast", SCREENCAST).await?;
    static SWEPT: AtomicBool = AtomicBool::new(false);
    if !SWEPT.swap(true, Ordering::AcqRel) {
        for orphan in sessions::orphans() {
            quickshell::qs_log!(debug, LOG, "Stopping {orphan}, left by a shell that died.");
            stop(&conn, &orphan).await;
        }
    }
    let path: OwnedObjectPath = sc.call("CreateSession", &(HashMap::<&str, Value>::new(),)).await?;
    sessions::remember(path.as_str());
    *session.path.lock().unwrap() = Some(path.clone());
    if session.dead.load(Ordering::Acquire) {
        session.shutdown();
        return Ok(());
    }
    let sp = zbus::Proxy::new(&conn, SCREENCAST, path.as_str(), "org.gnome.Mutter.ScreenCast.Session").await?;
    let props = HashMap::from([("window-id", Value::U64(window)), ("cursor-mode", Value::U32(cursor))]);
    let stream: OwnedObjectPath = sp.call("RecordWindow", &(props,)).await?;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.gnome.Mutter.ScreenCast.Stream")?
        .member("PipeWireStreamAdded")?
        .path(stream.as_str().to_owned())?
        .build();
    let mut added = qs_dbus::signals(&conn, rule).await?;
    sp.call::<_, _, ()>("Start", &()).await?;
    let Some(Ok(msg)) = qs_dbus::rt::next(&mut added).await else { return Ok(()) };
    let (node,): (u32,) = msg.body().deserialize()?;
    if !session.dead.load(Ordering::Acquire) {
        send(Cmd::Open { id: session.id, node, frames, live: session.live.clone() });
    }
    Ok(())
}

impl cxx_qt::Initialize for qobject::WindowCapture {
    fn initialize(mut self: Pin<&mut Self>) {
        *self.shared.qt.lock().unwrap() = Some(self.qt_thread());
        // The initial QML assignments coalesce into one session.
        self.as_mut().on_window_id_changed(|this| this.queue_update()).release();
        self.as_mut().on_app_id_changed(|this| this.queue_update()).release();
        self.as_mut().on_paint_cursor_changed(|this| this.queue_update()).release();
        self.as_mut()
            .on_live_changed(|this| {
                if let Some(s) = &this.session {
                    s.live.store(this.live, Ordering::Release);
                }
                if this.live {
                    this.capture_frame();
                }
            })
            .release();
        self.queue_update();
    }
}

impl Drop for WindowCaptureRust {
    fn drop(&mut self) {
        if let Some(s) = self.session.take() {
            s.shutdown();
        }
        *self.shared.qt.lock().unwrap() = None;
    }
}

impl qobject::WindowCapture {
    fn queue_update(mut self: Pin<&mut Self>) {
        if self.update_queued {
            return;
        }
        self.as_mut().rust_mut().update_queued = true;
        let _ = self.qt_thread().queue(|this| this.update());
    }

    fn update(mut self: Pin<&mut Self>) {
        {
            let mut rust = self.as_mut().rust_mut();
            rust.update_queued = false;
            if let Some(s) = rust.session.take() {
                s.shutdown();
            }
        }
        self.as_mut().clear();
        // The title only picks among twins at start: a terminal retitles on every command, so a
        // title change never restarts the recording.
        let target = if self.window_id >= 0 {
            Target::Id(self.window_id as u64)
        } else if !self.app_id.is_empty() {
            Target::Match { app_id: self.app_id.to_string(), title: self.title.to_string() }
        } else {
            return;
        };
        let session = Arc::new(Session {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            dead: AtomicBool::new(false),
            path: Mutex::new(None),
            live: Arc::new(AtomicBool::new(self.live)),
        });
        self.as_mut().rust_mut().session = Some(session.clone());
        let (cursor, frames) = (if self.paint_cursor { 1 } else { 0 }, self.shared.clone());
        let qt = self.qt_thread();
        qs_dbus::rt::spawn(async move {
            let id = session.id;
            if let Err(e) = start(session, target, cursor, frames).await {
                quickshell::qs_log!(warn, LOG, "Cannot record the window: {e}");
                let _ = qt.queue(move |mut this| {
                    if this.session.as_ref().is_some_and(|s| s.id == id) {
                        this.as_mut().stop_from_stream();
                    }
                });
            }
        });
    }

    fn capture_frame(self: Pin<&mut Self>) -> bool {
        let Some(s) = &self.session else { return false };
        send(Cmd::Resume(s.id));
        true
    }

    fn stop_from_stream(mut self: Pin<&mut Self>) {
        if let Some(s) = self.as_mut().rust_mut().session.take() {
            s.shutdown();
        }
        self.as_mut().clear();
        self.stopped();
    }

    fn clear(mut self: Pin<&mut Self>) {
        if self.has_content {
            self.as_mut().rust_mut().has_content = false;
            self.as_mut().has_content_changed();
        }
        if self.buffer_size != QSize::new(0, 0) {
            self.as_mut().rust_mut().buffer_size = QSize::new(0, 0);
            self.as_mut().buffer_size_changed();
        }
        self.shared.state.lock().unwrap().format = None;
    }
}
