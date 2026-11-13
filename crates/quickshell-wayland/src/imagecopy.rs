//! ScreencopyView capture of single windows over ext-image-copy-capture-v1 with
//! ext-foreign-toplevel-image-capture-source-v1 (Umbriel; niri after 26.04). Upstream does this
//! only for outputs and captures windows only on Hyprland. The compositor copies the window into
//! DMA-BUFs allocated here with GBM on the device it names, and waits for the window to change
//! before each copy; frames land in the same `Frames` queue the ScreenCast path fills, so
//! QsWindowCastItem draws both without a copy through the CPU.
//!
//! Pacing is upstream's: one frame in flight, the next asked when the item took the last one, so
//! a view nobody paints asks for nothing. At most three buffers: shown, waiting, being written.
//! Each buffer remembers what changed since it was last written, so the compositor copies only
//! that (the damage the protocol reports).

use crate::castffi::ffi as castffi;
use crate::windowcast::{Format, Frames, Planes, Target, KEY_SHIFT};
use qs_wayland::wayland_client::globals::{registry_queue_init, GlobalListContents};
use qs_wayland::wayland_client::protocol::{wl_buffer::WlBuffer, wl_registry};
use qs_wayland::wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use qs_wayland::wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self as handle_v1, ExtForeignToplevelHandleV1 as Handle},
    ext_foreign_toplevel_list_v1::{self as list_v1, ExtForeignToplevelListV1 as List},
};
use qs_wayland::wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1 as SourceManager,
    ext_image_capture_source_v1::ExtImageCaptureSourceV1 as Source,
};
use qs_wayland::wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self as frame_v1, ExtImageCopyCaptureFrameV1 as CaptureFrame},
    ext_image_copy_capture_manager_v1::{self as manager_v1, ExtImageCopyCaptureManagerV1 as Manager},
    ext_image_copy_capture_session_v1::{self as session_v1, ExtImageCopyCaptureSessionV1 as CaptureSession},
};
use qs_wayland::wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1::{self as params_v1, ZwpLinuxBufferParamsV1 as Params},
    zwp_linux_dmabuf_v1::{self as dmabuf_v1, ZwpLinuxDmabufV1 as Dmabuf},
};
use std::os::fd::{AsFd, AsRawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

const LOG: &str = "quickshell.wayland.imagecopy";
/// Marks a buffer key as this backend's: `cast_release` and `cast_take` route on it.
pub(crate) const KEY_FLAG: u64 = 1 << 63;
const MAX_BUFFERS: usize = 3;
const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
const DRM_FORMAT_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

struct State;

struct Globals {
    conn: Connection,
    qh: QueueHandle<State>,
    manager: Manager,
    sources: SourceManager,
    dmabuf: Dmabuf,
    _list: List,
}

/// The capture globals on their own queue, or None when the compositor lacks any of them (niri
/// 26.04) or Qt is not on Wayland. Qt thread only on the first call.
fn globals() -> Option<&'static Globals> {
    static G: OnceLock<Option<Globals>> = OnceLock::new();
    G.get_or_init(|| {
        let conn = qs_wayland::connection()?;
        let (list, mut queue) = registry_queue_init::<State>(&conn).ok()?;
        let offered = list.contents().clone_list();
        let has = |name: &str| offered.iter().any(|g| g.interface == name);
        if ![Manager::interface().name, SourceManager::interface().name, List::interface().name, Dmabuf::interface().name].into_iter().all(|n| has(n)) {
            quickshell::qs_log!(debug, LOG, "The compositor offers no ext-image-copy-capture of toplevels.");
            return None;
        }
        let qh = queue.handle();
        let manager = list.bind::<Manager, _, _>(&qh, 1..=1, ()).ok()?;
        let sources = list.bind::<SourceManager, _, _>(&qh, 1..=1, ()).ok()?;
        let dmabuf = list.bind::<Dmabuf, _, _>(&qh, 2..=4, ()).ok()?;
        let toplevels = list.bind::<List, _, _>(&qh, 1..=1, ()).ok()?;
        // Every open window is known before the first session looks for one.
        let _ = queue.roundtrip(&mut State);
        qs_wayland::spawn_dispatch("wayland-imagecopy", queue, State);
        Some(Globals { conn, qh, manager, sources, dmabuf, _list: toplevels })
    })
    .as_ref()
}

/// Whether windows can be captured here (the compositor answers through its globals).
pub(crate) fn available() -> bool {
    globals().is_some()
}

// --- Windows (ext-foreign-toplevel-list) ---

#[derive(Default)]
struct Window {
    identifier: String,
    app_id: String,
    title: String,
    ready: bool,
}

type WindowInfo = Arc<Mutex<Window>>;

/// Handles in the order the windows opened.
static WINDOWS: Mutex<Vec<Handle>> = Mutex::new(Vec::new());
/// Sessions whose window is not known yet (it is opening).
static WAITING: Mutex<Vec<Weak<Session>>> = Mutex::new(Vec::new());

fn info(h: &Handle) -> Option<std::sync::MutexGuard<'_, Window>> {
    h.data::<WindowInfo>().map(|w| w.lock().unwrap())
}

fn find(target: &Target) -> Option<Handle> {
    let windows = WINDOWS.lock().unwrap();
    match target {
        Target::Id(id) => windows.iter().find(|h| info(h).is_some_and(|w| w.ready && w.identifier == *id)).cloned(),
        // Twins (same app and title) resolve to the oldest, as on the ScreenCast path.
        Target::Match { app_id, title } => {
            let mut oldest = None;
            for h in windows.iter() {
                let Some(w) = info(h) else { continue };
                if !w.ready || w.app_id != *app_id {
                    continue;
                }
                if w.title == *title {
                    return Some(h.clone());
                }
                oldest.get_or_insert_with(|| h.clone());
            }
            oldest
        }
    }
}

fn wake_waiting() {
    let waiting = std::mem::take(&mut *WAITING.lock().unwrap());
    let still: Vec<_> = waiting.into_iter().filter(|w| w.upgrade().is_some_and(|s| !s.dead.load(Ordering::Acquire) && !s.connect())).collect();
    WAITING.lock().unwrap().extend(still);
}

// --- Sessions ---

static SESSIONS: Mutex<Vec<(u64, Weak<Session>)>> = Mutex::new(Vec::new());

fn session(key: u64) -> Option<Arc<Session>> {
    let id = (key & !KEY_FLAG) >> KEY_SHIFT;
    SESSIONS.lock().unwrap().iter().find(|(i, _)| *i == id).and_then(|(_, w)| w.upgrade())
}

#[derive(Clone, Copy, PartialEq)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Rect {
    fn union(self, o: Rect) -> Rect {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        let (r, b) = (self.x.saturating_add(self.w).max(o.x.saturating_add(o.w)), self.y.saturating_add(self.h).max(o.y.saturating_add(o.h)));
        Rect { x, y, w: r.saturating_sub(x), h: b.saturating_sub(y) }
    }
}

fn add(a: Option<Rect>, b: Rect) -> Option<Rect> {
    Some(a.map_or(b, |a| a.union(b)))
}

#[derive(Clone, Default, PartialEq)]
struct Constraints {
    width: u32,
    height: u32,
    device: Option<u64>,
    formats: Vec<(u32, Vec<u64>)>,
}

#[derive(PartialEq)]
enum Use {
    Free,
    Writing,
    /// Waiting in `Frames::pending` or on screen in the item.
    Shown,
}

struct Buffer {
    key: u64,
    wl: WlBuffer,
    _bo: gbm::Bo,
    state: Use,
    /// What changed since this buffer was last written; None when it is current.
    stale: Option<Rect>,
    /// From older constraints: destroyed once nobody uses it.
    retired: bool,
}

impl Drop for Buffer {
    fn drop(&mut self) {
        self.wl.destroy();
    }
}

struct InFlight {
    frame: CaptureFrame,
    key: u64,
    damage: Option<Rect>,
}

#[derive(Default)]
struct Inner {
    session: Option<CaptureSession>,
    source: Option<Source>,
    next: Constraints,
    current: Option<Constraints>,
    buffers: Vec<Buffer>,
    frame: Option<InFlight>,
    /// A capture was asked while none could start (no constraints or no free buffer yet).
    wanted: bool,
    shown_once: bool,
    seq: u64,
    failures: u8,
}

pub(crate) struct Session {
    pub(crate) id: u64,
    frames: Arc<Frames>,
    target: Target,
    cursor: bool,
    live: AtomicBool,
    dead: AtomicBool,
    inner: Mutex<Inner>,
}

/// Starts capturing `target` into `frames`; frames carry `id` in their keys.
pub(crate) fn open(id: u64, target: Target, cursor: bool, live: bool, frames: Arc<Frames>) -> Arc<Session> {
    let s = Arc::new(Session { id, frames, target, cursor, live: AtomicBool::new(live), dead: AtomicBool::new(false), inner: Mutex::new(Inner { wanted: true, ..Default::default() }) });
    SESSIONS.lock().unwrap().push((id, Arc::downgrade(&s)));
    // Held across the attempt, so a window listed meanwhile still wakes this session.
    let mut waiting = WAITING.lock().unwrap();
    if !s.connect() {
        quickshell::qs_log!(debug, LOG, "Window not listed yet; capturing it once it is.");
        waiting.push(Arc::downgrade(&s));
    }
    drop(waiting);
    s
}

/// The item took `key` off the queue (it paints it now): a live view asks for the next frame.
pub(crate) fn taken(key: u64) {
    if let Some(s) = session(key)
        && s.live.load(Ordering::Acquire) {
            let mut inner = s.inner.lock().unwrap();
            s.start(&mut inner);
        }
}

/// The item is done with `key`: free for the compositor to write again.
pub(crate) fn release(key: u64) {
    let Some(s) = session(key) else { return };
    let mut inner = s.inner.lock().unwrap();
    let Some(i) = inner.buffers.iter().position(|b| b.key == key) else { return };
    if inner.buffers[i].retired {
        s.drop_buffer(&mut inner, i);
    } else {
        inner.buffers[i].state = Use::Free;
    }
    if inner.wanted {
        s.start(&mut inner);
    }
}

impl Session {
    /// Opens the compositor session once the window is listed; false while it is not.
    fn connect(self: &Arc<Self>) -> bool {
        let Some(handle) = find(&self.target) else { return false };
        let Some(g) = globals() else { return true };
        let mut inner = self.inner.lock().unwrap();
        if self.dead.load(Ordering::Acquire) || inner.session.is_some() {
            return true;
        }
        let source = g.sources.create_source(&handle, &g.qh, ());
        let options = if self.cursor { manager_v1::Options::PaintCursors } else { manager_v1::Options::empty() };
        inner.session = Some(g.manager.create_session(&source, options, &g.qh, Arc::downgrade(self)));
        inner.source = Some(source);
        drop(inner);
        let _ = g.conn.flush();
        true
    }

    pub(crate) fn set_live(self: &Arc<Self>, live: bool) {
        self.live.store(live, Ordering::Release);
        let mut inner = self.inner.lock().unwrap();
        if live {
            self.start(&mut inner);
        } else if inner.shown_once
            && let Some(f) = inner.frame.take() {
                // A paused view keeps its last picture and costs the compositor nothing more.
                f.frame.destroy();
                self.written_off(&mut inner, f.key);
            }
    }

    /// One more frame (a paused view's refresh).
    pub(crate) fn capture(self: &Arc<Self>) {
        let mut inner = self.inner.lock().unwrap();
        self.start(&mut inner);
    }

    pub(crate) fn shutdown(&self) {
        if self.dead.swap(true, Ordering::AcqRel) {
            return;
        }
        SESSIONS.lock().unwrap().retain(|(id, _)| *id != self.id);
        let mut inner = self.inner.lock().unwrap();
        if let Some(f) = inner.frame.take() {
            f.frame.destroy();
        }
        if let Some(s) = inner.session.take() {
            s.destroy();
        }
        if let Some(s) = inner.source.take() {
            s.destroy();
        }
        // The item's texture keeps its own reference to the memory; the rest goes now.
        while !inner.buffers.is_empty() {
            self.drop_buffer(&mut inner, 0);
        }
        if let Some(g) = globals() {
            let _ = g.conn.flush();
        }
    }

    /// The compositor or a failure ended the capture: the view goes back to no content.
    fn end(&self) {
        let id = self.id;
        self.frames.ended(id);
    }

    fn drop_buffer(&self, inner: &mut Inner, i: usize) {
        let buffer = inner.buffers.remove(i);
        let mut state = self.frames.state.lock().unwrap();
        state.buffers.remove(&buffer.key);
        if state.pending == Some(buffer.key) {
            state.pending = None;
        }
    }

    /// A buffer the compositor may have half written: free, and whole to redo.
    fn written_off(&self, inner: &mut Inner, key: u64) {
        let Some(i) = inner.buffers.iter().position(|b| b.key == key) else { return };
        if inner.buffers[i].retired {
            self.drop_buffer(inner, i);
        } else {
            let b = &mut inner.buffers[i];
            b.state = Use::Free;
            b.stale = Some(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
        }
    }

    /// Asks for the next frame unless one is in flight; remembers the ask when it cannot yet.
    fn start(self: &Arc<Self>, inner: &mut Inner) {
        if self.dead.load(Ordering::Acquire) || inner.frame.is_some() {
            return;
        }
        let (Some(session), Some(c), Some(g)) = (inner.session.clone(), inner.current.clone(), globals()) else {
            inner.wanted = true;
            return;
        };
        let i = match inner.buffers.iter().position(|b| b.state == Use::Free && !b.retired) {
            Some(i) => i,
            None if inner.buffers.iter().filter(|b| !b.retired).count() < MAX_BUFFERS => match self.allocate(inner, &c, g) {
                Some(i) => i,
                None => {
                    self.end();
                    return;
                }
            },
            None => {
                inner.wanted = true;
                return;
            }
        };
        inner.wanted = false;
        let b = &mut inner.buffers[i];
        let key = b.key;
        let frame = session.create_frame(&g.qh, FrameOwner { session: Arc::downgrade(self), key });
        frame.attach_buffer(&b.wl);
        if let Some(r) = b.stale.take() {
            // Clipped to the buffer: the protocol refuses an empty or negative rectangle.
            let (x, y) = (r.x.max(0), r.y.max(0));
            let (right, bottom) = (r.x.saturating_add(r.w).min(c.width as i32), r.y.saturating_add(r.h).min(c.height as i32));
            if right > x && bottom > y {
                frame.damage_buffer(x, y, right - x, bottom - y);
            }
        }
        frame.capture();
        b.state = Use::Writing;
        inner.frame = Some(InFlight { frame, key, damage: None });
        let _ = g.conn.flush();
    }

    fn allocate(&self, inner: &mut Inner, c: &Constraints, g: &Globals) -> Option<usize> {
        let Some(device) = c.device else {
            quickshell::qs_log!(warn, LOG, "The compositor offers window frames only in shared memory; qs-rs draws them from DMA-BUFs.");
            return None;
        };
        let pick = [DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888].into_iter().find_map(|fourcc| {
            let offered = &c.formats.iter().find(|f| f.0 == fourcc)?.1;
            let importable = importable(fourcc);
            let explicit: Vec<u64> = offered.iter().copied().filter(|m| *m != DRM_FORMAT_MOD_INVALID && importable.contains(m)).collect();
            let implicit = offered.contains(&DRM_FORMAT_MOD_INVALID) && importable.contains(&DRM_FORMAT_MOD_INVALID);
            (!explicit.is_empty() || implicit).then_some((fourcc, explicit, implicit))
        });
        let Some((fourcc, modifiers, implicit)) = pick else {
            quickshell::qs_log!(warn, LOG, "No window frame format both the compositor writes and EGL reads.");
            return None;
        };
        let Some(bo) = gbm::allocate(device, c.width, c.height, fourcc, &modifiers, implicit) else {
            quickshell::qs_log!(warn, LOG, "Cannot allocate a {}x{} buffer for window capture.", c.width, c.height);
            return None;
        };
        let params = g.dmabuf.create_params(&g.qh, ());
        let mut planes = Planes { count: bo.fds.len() as u32, format: Some(Format { width: c.width, height: c.height, fourcc, modifier: bo.modifier }), ..Default::default() };
        for (i, fd) in bo.fds.iter().enumerate() {
            params.add(fd.as_fd(), i as u32, bo.offsets[i], bo.strides[i], (bo.modifier >> 32) as u32, bo.modifier as u32);
            planes.fds[i] = fd.as_raw_fd();
            planes.offsets[i] = bo.offsets[i];
            planes.strides[i] = bo.strides[i];
        }
        let wl = params.create_immed(c.width as i32, c.height as i32, fourcc, params_v1::Flags::empty(), &g.qh, ());
        params.destroy();
        inner.seq += 1;
        let key = KEY_FLAG | (self.id << KEY_SHIFT) | inner.seq;
        self.frames.state.lock().unwrap().buffers.insert(key, planes);
        let full = Rect { x: 0, y: 0, w: c.width as i32, h: c.height as i32 };
        inner.buffers.push(Buffer { key, wl, _bo: bo, state: Use::Free, stale: Some(full), retired: false });
        Some(inner.buffers.len() - 1)
    }

    /// New constraints: buffers made for the old ones stop being offered and go once unused.
    fn retire(&self, inner: &mut Inner) {
        let mut i = 0;
        while i < inner.buffers.len() {
            let b = &mut inner.buffers[i];
            b.retired = true;
            let pending = self.frames.state.lock().unwrap().pending == Some(b.key);
            if b.state == Use::Free || (b.state == Use::Shown && pending) {
                self.drop_buffer(inner, i);
                continue;
            }
            // Shown in the item or being written: no longer offered to the item's cache.
            self.frames.state.lock().unwrap().buffers.remove(&inner.buffers[i].key);
            i += 1;
        }
    }

    fn constraints_done(self: &Arc<Self>) {
        let mut inner = self.inner.lock().unwrap();
        let next = std::mem::take(&mut inner.next);
        if inner.current.as_ref() == Some(&next) {
            return;
        }
        let resized = inner.current.as_ref().is_none_or(|c| c.width != next.width || c.height != next.height);
        self.retire(&mut inner);
        let (w, h) = (next.width, next.height);
        inner.current = Some(next);
        inner.failures = 0;
        if resized {
            self.frames.resized(self.id, w, h);
        }
        if inner.wanted {
            self.start(&mut inner);
        }
    }

    fn ready(self: &Arc<Self>, key: u64) {
        let mut inner = self.inner.lock().unwrap();
        let Some(f) = inner.frame.take_if(|f| f.key == key) else { return };
        f.frame.destroy();
        inner.failures = 0;
        let full = inner.current.as_ref().map_or(Rect { x: 0, y: 0, w: 0, h: 0 }, |c| Rect { x: 0, y: 0, w: c.width as i32, h: c.height as i32 });
        let damage = f.damage.unwrap_or(full);
        for b in inner.buffers.iter_mut().filter(|b| b.key != key) {
            b.stale = add(b.stale, damage);
        }
        let Some(i) = inner.buffers.iter().position(|b| b.key == key) else { return };
        if inner.buffers[i].retired {
            self.drop_buffer(&mut inner, i);
            self.start(&mut inner);
            return;
        }
        inner.buffers[i].state = Use::Shown;
        inner.buffers[i].stale = None;
        inner.shown_once = true;
        let previous = self.frames.state.lock().unwrap().pending.replace(key);
        // A frame the item never took: free again.
        if let Some(old) = previous.filter(|old| *old != key)
            && let Some(j) = inner.buffers.iter().position(|b| b.key == old) {
                inner.buffers[j].state = Use::Free;
            }
        drop(inner);
        self.frames.frame_arrived();
    }

    fn failed(self: &Arc<Self>, key: u64, reason: WEnum<frame_v1::FailureReason>) {
        let mut inner = self.inner.lock().unwrap();
        let Some(f) = inner.frame.take_if(|f| f.key == key) else { return };
        f.frame.destroy();
        self.written_off(&mut inner, key);
        match reason {
            // The session's stopped event ends it.
            WEnum::Value(frame_v1::FailureReason::Stopped) => {}
            // Buffers out of date: new ones, once; a compositor that keeps refusing ends it.
            WEnum::Value(frame_v1::FailureReason::BufferConstraints) if inner.failures == 0 => {
                inner.failures += 1;
                self.retire(&mut inner);
                self.start(&mut inner);
            }
            _ => {
                quickshell::qs_log!(warn, LOG, "Ending window capture after a failed frame.");
                drop(inner);
                self.end();
            }
        }
    }
}

/// The session and buffer a frame in flight writes.
struct FrameOwner {
    session: Weak<Session>,
    key: u64,
}

/// Modifiers Qt's EGL imports as a 2D texture for `fourcc` (DRM_FORMAT_MOD_INVALID = implicit).
fn importable(fourcc: u32) -> Vec<u64> {
    static CACHE: Mutex<Vec<(u32, Vec<u64>)>> = Mutex::new(Vec::new());
    let mut cache = CACHE.lock().unwrap();
    if let Some((_, m)) = cache.iter().find(|(f, _)| *f == fourcc) {
        return m.clone();
    }
    let m = castffi::dmabuf_modifiers(fourcc);
    cache.push((fourcc, m.clone()));
    m
}

// --- Dispatch ---

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<Manager, ()> for State {
    fn event(_: &mut Self, _: &Manager, _: manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<SourceManager, ()> for State {
    fn event(_: &mut Self, _: &SourceManager, _: <SourceManager as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<Source, ()> for State {
    fn event(_: &mut Self, _: &Source, _: <Source as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<Dmabuf, ()> for State {
    fn event(_: &mut Self, _: &Dmabuf, _: dmabuf_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<Params, ()> for State {
    fn event(_: &mut Self, _: &Params, event: params_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let params_v1::Event::Failed = event {
            quickshell::qs_log!(warn, LOG, "The compositor refused a window capture buffer.");
        }
    }
    // Only `create` answers with a buffer; `create_immed` never does.
    event_created_child!(State, Params, [
        params_v1::EVT_CREATED_OPCODE => (WlBuffer, ()),
    ]);
}
impl Dispatch<WlBuffer, ()> for State {
    fn event(_: &mut Self, _: &WlBuffer, _: <WlBuffer as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<List, ()> for State {
    fn event(_: &mut Self, _: &List, event: list_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let list_v1::Event::Toplevel { toplevel } = event {
            WINDOWS.lock().unwrap().push(toplevel);
        }
    }
    event_created_child!(State, List, [
        list_v1::EVT_TOPLEVEL_OPCODE => (Handle, WindowInfo::default()),
    ]);
}

impl Dispatch<Handle, WindowInfo> for State {
    fn event(_: &mut Self, handle: &Handle, event: handle_v1::Event, info: &WindowInfo, _: &Connection, _: &QueueHandle<Self>) {
        use handle_v1::Event as E;
        match event {
            E::Identifier { identifier } => info.lock().unwrap().identifier = identifier,
            E::AppId { app_id } => info.lock().unwrap().app_id = app_id,
            E::Title { title } => info.lock().unwrap().title = title,
            E::Done => {
                let first = !std::mem::replace(&mut info.lock().unwrap().ready, true);
                if first {
                    wake_waiting();
                }
            }
            E::Closed => {
                WINDOWS.lock().unwrap().retain(|h| h != handle);
                handle.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<CaptureSession, Weak<Session>> for State {
    fn event(_: &mut Self, _: &CaptureSession, event: session_v1::Event, session: &Weak<Session>, _: &Connection, _: &QueueHandle<Self>) {
        use session_v1::Event as E;
        let Some(s) = session.upgrade().filter(|s| !s.dead.load(Ordering::Acquire)) else { return };
        match event {
            E::BufferSize { width, height } => {
                let mut inner = s.inner.lock().unwrap();
                inner.next.width = width;
                inner.next.height = height;
            }
            E::DmabufDevice { device } => {
                if let Ok(bytes) = <[u8; 8]>::try_from(device.as_slice()) {
                    s.inner.lock().unwrap().next.device = Some(u64::from_ne_bytes(bytes));
                }
            }
            E::DmabufFormat { format, modifiers } => {
                let mods = modifiers.chunks_exact(8).map(|c| u64::from_ne_bytes(c.try_into().unwrap())).collect();
                s.inner.lock().unwrap().next.formats.push((format, mods));
            }
            E::Done => s.constraints_done(),
            E::Stopped => {
                quickshell::qs_log!(debug, LOG, "The compositor ended a window capture (the window closed).");
                s.end();
            }
            _ => {}
        }
    }
}

impl Dispatch<CaptureFrame, FrameOwner> for State {
    fn event(_: &mut Self, _: &CaptureFrame, event: frame_v1::Event, owner: &FrameOwner, _: &Connection, _: &QueueHandle<Self>) {
        use frame_v1::Event as E;
        let Some(s) = owner.session.upgrade().filter(|s| !s.dead.load(Ordering::Acquire)) else { return };
        let key = owner.key;
        match event {
            E::Damage { x, y, width, height } => {
                let mut inner = s.inner.lock().unwrap();
                if let Some(f) = inner.frame.as_mut().filter(|f| f.key == key) {
                    f.damage = add(f.damage, Rect { x, y, w: width, h: height });
                }
            }
            E::Ready => s.ready(key),
            E::Failed { reason } => s.failed(key, reason),
            _ => {}
        }
    }
}

/// GBM through dlopen: Mesa ships it everywhere qs-rs runs, but a missing one costs only window
/// capture, never the shell's start.
mod gbm {
    use std::ffi::c_void;
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::sync::{Mutex, OnceLock};

    const USE_RENDERING: u32 = 1 << 2;
    const MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

    struct Lib {
        create_device: unsafe extern "C" fn(i32) -> *mut c_void,
        bo_create: unsafe extern "C" fn(*mut c_void, u32, u32, u32, u32) -> *mut c_void,
        bo_create_with_modifiers2: Option<unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const u64, u32, u32) -> *mut c_void>,
        bo_create_with_modifiers: Option<unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const u64, u32) -> *mut c_void>,
        bo_get_plane_count: unsafe extern "C" fn(*mut c_void) -> i32,
        bo_get_fd_for_plane: unsafe extern "C" fn(*mut c_void, i32) -> i32,
        bo_get_stride_for_plane: unsafe extern "C" fn(*mut c_void, i32) -> u32,
        bo_get_offset: unsafe extern "C" fn(*mut c_void, i32) -> u32,
        bo_get_modifier: unsafe extern "C" fn(*mut c_void) -> u64,
        bo_destroy: unsafe extern "C" fn(*mut c_void),
    }

    fn lib() -> Option<&'static Lib> {
        static L: OnceLock<Option<Lib>> = OnceLock::new();
        L.get_or_init(|| unsafe {
            let h = libc::dlopen(c"libgbm.so.1".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if h.is_null() {
                quickshell::qs_log!(warn, super::LOG, "libgbm.so.1 is missing; windows cannot be captured.");
                return None;
            }
            macro_rules! sym {
                ($name:literal) => {{
                    let p = libc::dlsym(h, $name.as_ptr());
                    if p.is_null() { None } else { Some(std::mem::transmute::<*mut c_void, _>(p)) }
                }};
            }
            Some(Lib {
                create_device: sym!(c"gbm_create_device")?,
                bo_create: sym!(c"gbm_bo_create")?,
                bo_create_with_modifiers2: sym!(c"gbm_bo_create_with_modifiers2"),
                bo_create_with_modifiers: sym!(c"gbm_bo_create_with_modifiers"),
                bo_get_plane_count: sym!(c"gbm_bo_get_plane_count")?,
                bo_get_fd_for_plane: sym!(c"gbm_bo_get_fd_for_plane")?,
                bo_get_stride_for_plane: sym!(c"gbm_bo_get_stride_for_plane")?,
                bo_get_offset: sym!(c"gbm_bo_get_offset")?,
                bo_get_modifier: sym!(c"gbm_bo_get_modifier")?,
                bo_destroy: sym!(c"gbm_bo_destroy")?,
            })
        })
        .as_ref()
    }

    struct Device {
        dev: u64,
        ptr: *mut c_void,
        _fd: OwnedFd,
    }

    // GBM calls are serialized by DEVICES; the pointers never leave it.
    unsafe impl Send for Device {}
    struct Devices(Vec<Device>);
    static DEVICES: Mutex<Devices> = Mutex::new(Devices(Vec::new()));

    /// glibc's dev_t layout.
    fn split(dev: u64) -> (u64, u64) {
        (((dev >> 32) & 0xffff_f000) | ((dev >> 8) & 0xfff), ((dev >> 12) & 0xffff_ff00) | (dev & 0xff))
    }

    /// The render node of the device the compositor named (it may name the primary node), or the
    /// node itself.
    fn node(dev: u64) -> Option<std::path::PathBuf> {
        use std::os::unix::fs::MetadataExt;
        let (major, minor) = split(dev);
        if let Ok(entries) = std::fs::read_dir(format!("/sys/dev/char/{major}:{minor}/device/drm")) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("renderD") {
                    return Some(std::path::Path::new("/dev/dri").join(e.file_name()));
                }
            }
        }
        std::fs::read_dir("/dev/dri").ok()?.flatten().map(|e| e.path()).find(|p| std::fs::metadata(p).is_ok_and(|m| m.rdev() == dev))
    }

    fn device(devices: &mut Devices, dev: u64, lib: &Lib) -> Option<*mut c_void> {
        if let Some(d) = devices.0.iter().find(|d| d.dev == dev) {
            return Some(d.ptr);
        }
        let path = node(dev)?;
        let file = std::fs::OpenOptions::new().read(true).write(true).open(&path).ok()?;
        let fd: OwnedFd = file.into();
        let ptr = unsafe { (lib.create_device)(std::os::fd::AsRawFd::as_raw_fd(&fd)) };
        if ptr.is_null() {
            return None;
        }
        devices.0.push(Device { dev, ptr, _fd: fd });
        Some(ptr)
    }

    pub struct Bo {
        ptr: *mut c_void,
        pub fds: Vec<OwnedFd>,
        pub offsets: [u32; 4],
        pub strides: [u32; 4],
        pub modifier: u64,
    }

    unsafe impl Send for Bo {}

    impl Drop for Bo {
        fn drop(&mut self) {
            let _devices = DEVICES.lock().unwrap();
            if let Some(lib) = lib() {
                unsafe { (lib.bo_destroy)(self.ptr) };
            }
        }
    }

    /// A buffer the GPU renders into, laid out with one of `modifiers`, or implicitly when only
    /// that is allowed.
    pub fn allocate(dev: u64, width: u32, height: u32, fourcc: u32, modifiers: &[u64], implicit: bool) -> Option<Bo> {
        let lib = lib()?;
        let mut devices = DEVICES.lock().unwrap();
        let gbm = device(&mut devices, dev, lib)?;
        let (ptr, modifier) = unsafe {
            let explicit = if modifiers.is_empty() {
                std::ptr::null_mut()
            } else if let Some(f) = lib.bo_create_with_modifiers2 {
                f(gbm, width, height, fourcc, modifiers.as_ptr(), modifiers.len() as u32, USE_RENDERING)
            } else if let Some(f) = lib.bo_create_with_modifiers {
                f(gbm, width, height, fourcc, modifiers.as_ptr(), modifiers.len() as u32)
            } else {
                std::ptr::null_mut()
            };
            if !explicit.is_null() {
                (explicit, (lib.bo_get_modifier)(explicit))
            } else if implicit {
                ((lib.bo_create)(gbm, width, height, fourcc, USE_RENDERING), MOD_INVALID)
            } else {
                (std::ptr::null_mut(), MOD_INVALID)
            }
        };
        if ptr.is_null() {
            return None;
        }
        let mut bo = Bo { ptr, fds: Vec::new(), offsets: [0; 4], strides: [0; 4], modifier };
        let count = unsafe { (lib.bo_get_plane_count)(ptr) }.clamp(0, 4);
        for i in 0..count {
            let fd = unsafe { (lib.bo_get_fd_for_plane)(ptr, i) };
            if fd < 0 {
                drop(devices);
                return None;
            }
            bo.fds.push(unsafe { OwnedFd::from_raw_fd(fd) });
            bo.offsets[i as usize] = unsafe { (lib.bo_get_offset)(ptr, i) };
            bo.strides[i as usize] = unsafe { (lib.bo_get_stride_for_plane)(ptr, i) };
        }
        drop(devices);
        (count > 0).then_some(bo)
    }
}
