//! `FileView` core (`FileViewInternal`; FileView.qml wraps it so `text()`/`data()` are binding
//! aware). Reads and writes run on worker threads; a single live operation is tracked and its
//! result is applied on the Qt thread, or joined synchronously for blocking reads/writes.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
    }

    #[qenum(FileViewError)]
    enum Enum {
        Success,
        Unknown,
        FileNotFound,
        PermissionDenied,
        NotAFile,
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "FileViewError"]
        #[qml_uncreatable]
        type FileViewError = super::FileViewErrorRust;

        #[qinvokable]
        #[cxx_name = "toString"]
        fn to_string(self: &FileViewError, value: i32) -> QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "FileViewInternal"]
        #[qproperty(QString, _qsPath, READ = path, WRITE = set_path, NOTIFY = path_changed)]
        #[qproperty(QString, _qsText, READ = text, NOTIFY = internal_text_changed)]
        #[qproperty(QByteArray, _qsData, READ = data, NOTIFY = internal_data_changed)]
        #[qproperty(bool, _qsPreload, READ = preload, WRITE = set_preload, NOTIFY = preload_changed)]
        #[qproperty(bool, _qsBlockLoading, READ = block_loading, WRITE = set_block_loading, NOTIFY = block_loading_changed)]
        #[qproperty(bool, _qsBlockAllReads, READ = block_all_reads, WRITE = set_block_all_reads, NOTIFY = block_all_reads_changed)]
        #[qproperty(bool, _qsPrintErrors, READ = print_errors, WRITE = set_print_errors, NOTIFY = print_errors_changed)]
        #[qproperty(bool, loaded, READ = is_loaded_or_async, NOTIFY = loaded_or_async_changed)]
        #[qproperty(bool, block_writes, cxx_name = "blockWrites")]
        #[qproperty(bool, atomic_writes, cxx_name = "atomicWrites")]
        #[qproperty(bool, watch_changes, cxx_name = "watchChanges", READ, WRITE = set_watch_changes, NOTIFY)]
        #[qproperty(*mut QObject, adapter, READ, WRITE = set_adapter, NOTIFY)]
        type FileViewInternal = super::FileViewRust;

        fn path(self: &FileViewInternal) -> QString;
        fn set_path(self: Pin<&mut FileViewInternal>, path: QString);
        fn text(self: &FileViewInternal) -> QString;
        fn data(self: &FileViewInternal) -> QByteArray;
        fn preload(self: &FileViewInternal) -> bool;
        fn set_preload(self: Pin<&mut FileViewInternal>, preload: bool);
        fn block_loading(self: &FileViewInternal) -> bool;
        fn set_block_loading(self: Pin<&mut FileViewInternal>, value: bool);
        fn block_all_reads(self: &FileViewInternal) -> bool;
        fn set_block_all_reads(self: Pin<&mut FileViewInternal>, value: bool);
        fn print_errors(self: &FileViewInternal) -> bool;
        fn set_print_errors(self: Pin<&mut FileViewInternal>, value: bool);
        fn is_loaded_or_async(self: &FileViewInternal) -> bool;
        fn set_watch_changes(self: Pin<&mut FileViewInternal>, value: bool);
        unsafe fn set_adapter(self: Pin<&mut FileViewInternal>, adapter: *mut QObject);

        /// Called by FileView.qml before reading `_qsText`/`_qsData`: loads if not prepared yet.
        #[qinvokable]
        #[cxx_name = "_qsPrepareRead"]
        fn prepare_read(self: Pin<&mut FileViewInternal>, string: bool);
        #[qinvokable]
        #[cxx_name = "waitForJob"]
        fn wait_for_job(self: Pin<&mut FileViewInternal>) -> bool;
        #[qinvokable]
        fn reload(self: Pin<&mut FileViewInternal>);
        #[qinvokable]
        #[cxx_name = "setText"]
        fn set_text(self: Pin<&mut FileViewInternal>, text: &QString);
        #[qinvokable]
        #[cxx_name = "setData"]
        fn set_data(self: Pin<&mut FileViewInternal>, data: &QByteArray);
        #[qinvokable]
        #[cxx_name = "writeAdapter"]
        fn write_adapter(self: Pin<&mut FileViewInternal>);

        #[qsignal]
        #[cxx_name = "loaded"]
        fn loaded_signal(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "loadFailed"]
        fn load_failed(self: Pin<&mut FileViewInternal>, error: i32);
        #[qsignal]
        fn saved(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "saveFailed"]
        fn save_failed(self: Pin<&mut FileViewInternal>, error: i32);
        #[qsignal]
        #[cxx_name = "fileChanged"]
        fn file_changed(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "adapterUpdated"]
        fn adapter_updated(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "pathChanged"]
        fn path_changed(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "internalTextChanged"]
        fn internal_text_changed(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "internalDataChanged"]
        fn internal_data_changed(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "textChanged"]
        fn text_changed(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "preloadChanged"]
        fn preload_changed(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "loadedOrAsyncChanged"]
        fn loaded_or_async_changed(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "blockLoadingChanged"]
        fn block_loading_changed(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "blockAllReadsChanged"]
        fn block_all_reads_changed(self: Pin<&mut FileViewInternal>);
        #[qsignal]
        #[cxx_name = "printErrorsChanged"]
        fn print_errors_changed(self: Pin<&mut FileViewInternal>);
    }

    impl cxx_qt::Threading for FileViewInternal {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QString};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

#[derive(Default)]
pub struct FileViewErrorRust;

impl qobject::FileViewError {
    fn to_string(&self, value: i32) -> QString {
        QString::from(match value {
            0 => "Success",
            2 => "FileNotFound",
            3 => "PermissionDenied",
            4 => "NotAFile",
            _ => "Unknown",
        })
    }
}

#[derive(Default, Clone, PartialEq)]
pub struct State {
    pub path: String,
    pub data: Vec<u8>,
    pub exists: bool,
    pub error: i32,
}

#[derive(PartialEq)]
enum Kind {
    Read,
    Write,
}

struct Op {
    id: u64,
    kind: Kind,
    path: String,
    cancel: Arc<AtomicBool>,
    handle: JoinHandle<State>,
}

pub struct FileViewRust {
    target_path: String,
    preload: bool,
    block_loading: bool,
    block_all_reads: bool,
    print_errors: bool,
    block_writes: bool,
    atomic_writes: bool,
    watch_changes: bool,
    adapter: *mut cxx_qt::QObject,
    state: State,
    prepared: bool,
    loaded_or_async: bool,
    write_data: Option<Vec<u8>>,
    live: Option<Op>,
    next_id: u64,
    watcher: Option<crate::watch::Subscription>,
}

impl Default for FileViewRust {
    fn default() -> Self {
        Self {
            target_path: String::new(),
            preload: true,
            block_loading: false,
            block_all_reads: false,
            print_errors: true,
            block_writes: false,
            atomic_writes: true,
            watch_changes: false,
            adapter: std::ptr::null_mut(),
            state: State::default(),
            prepared: false,
            loaded_or_async: false,
            write_data: None,
            live: None,
            next_id: 0,
            watcher: None,
        }
    }
}

pub fn read(path: &str, print: bool, cancel: &AtomicBool) -> State {
    let mut state = State { path: path.to_string(), ..Default::default() };
    let warn = |msg: &str| {
        if print {
            eprintln!("FileView: Read of {path} failed: {msg}");
        }
    };
    let p = Path::new(path);
    state.exists = p.exists();
    if !state.exists {
        warn("File does not exist.");
        state.error = 2;
        return state;
    }
    if !p.is_file() {
        warn("Not a file.");
        state.error = 4;
        return state;
    }
    if cancel.load(Ordering::Acquire) {
        return state;
    }
    match std::fs::read(p) {
        Ok(data) => state.data = data,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            warn("Permission denied.");
            state.error = 3;
        }
        Err(_) => {
            warn("read() failed.");
            state.error = 1;
        }
    }
    state
}

pub fn write(path: &str, data: &[u8], atomic: bool, print: bool) -> State {
    let mut state = State { path: path.to_string(), data: data.to_vec(), ..Default::default() };
    let warn = |msg: &str| {
        if print {
            eprintln!("FileView: Write of {path} failed: {msg}");
        }
    };
    let p = Path::new(path);
    state.exists = p.exists();
    if !state.exists {
        if let Some(dir) = p.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                warn("Could not create parent directories of file.");
                state.error = 3;
                return state;
            }
        }
    }
    let result = if atomic {
        let tmp = p.with_file_name(format!(".{}.qsrs-tmp", p.file_name().unwrap_or_default().to_string_lossy()));
        std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, p)).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    } else {
        std::fs::write(p, data)
    };
    match result {
        Ok(()) => state.exists = true,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            warn("Permission denied.");
            state.error = 3;
        }
        Err(_) => {
            warn("Unknown error when writing file.");
            state.error = 1;
        }
    }
    state
}

impl qobject::FileViewInternal {
    fn path(&self) -> QString {
        QString::from(&self.state.path)
    }
    fn text(&self) -> QString {
        QString::from(&*String::from_utf8_lossy(&self.state.data))
    }
    fn data(&self) -> QByteArray {
        QByteArray::from(self.state.data.as_slice())
    }
    fn preload(&self) -> bool {
        self.preload
    }
    fn block_loading(&self) -> bool {
        self.block_loading
    }
    fn block_all_reads(&self) -> bool {
        self.block_all_reads
    }
    fn print_errors(&self) -> bool {
        self.print_errors
    }
    fn is_loaded_or_async(&self) -> bool {
        self.loaded_or_async
    }

    fn set_path(mut self: Pin<&mut Self>, path: QString) {
        let path = path.to_string();
        let p = path.strip_prefix("file://").unwrap_or(&path).to_string();
        if p == self.target_path {
            return;
        }
        if self.live.as_ref().is_some_and(|o| o.kind == Kind::Write) {
            self.as_mut().wait_for_job();
        } else {
            self.as_mut().cancel_async();
        }
        self.as_mut().rust_mut().target_path = p;
        self.update_path();
    }

    fn update_path(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().prepared = false;
        if self.target_path.is_empty() {
            self.as_mut().update_state(State::default());
        } else if self.preload {
            self.as_mut().load_async();
        } else {
            self.as_mut().emit_data_changed();
        }
        self.update_watcher();
    }

    fn should_block_read(&self) -> bool {
        self.block_all_reads || (self.block_loading && !self.loaded_or_async)
    }

    fn set_preload(mut self: Pin<&mut Self>, preload: bool) {
        if preload == self.preload {
            return;
        }
        self.as_mut().rust_mut().preload = preload;
        self.as_mut().preload_changed();
        if preload {
            self.as_mut().emit_data_changed();
            if !self.prepared {
                self.load_async();
            }
        }
    }

    fn set_block_loading(mut self: Pin<&mut Self>, value: bool) {
        if value == self.block_loading {
            return;
        }
        let was = self.should_block_read();
        self.as_mut().rust_mut().block_loading = value;
        self.as_mut().block_loading_changed();
        if !was && self.should_block_read() {
            self.emit_data_changed();
        }
    }

    fn set_block_all_reads(mut self: Pin<&mut Self>, value: bool) {
        if value == self.block_all_reads {
            return;
        }
        let was = self.should_block_read();
        self.as_mut().rust_mut().block_all_reads = value;
        self.as_mut().block_all_reads_changed();
        if !was && self.should_block_read() {
            self.emit_data_changed();
        }
    }

    fn set_print_errors(mut self: Pin<&mut Self>, value: bool) {
        if value != self.print_errors {
            self.as_mut().rust_mut().print_errors = value;
            self.print_errors_changed();
        }
    }

    fn set_watch_changes(mut self: Pin<&mut Self>, value: bool) {
        if value != self.watch_changes {
            self.as_mut().rust_mut().watch_changes = value;
            self.as_mut().watch_changes_changed();
            self.update_watcher();
        }
    }

    unsafe fn set_adapter(mut self: Pin<&mut Self>, adapter: *mut cxx_qt::QObject) {
        if adapter == self.adapter {
            return;
        }
        let me = unsafe { self.as_mut().get_unchecked_mut() } as *mut Self;
        if let Some(old) = unsafe { crate::jsonadapter::Adapter::from_ptr(self.adapter) } {
            old.attach(std::ptr::null_mut());
        }
        self.as_mut().rust_mut().adapter = adapter;
        if let Some(new) = unsafe { crate::jsonadapter::Adapter::from_ptr(adapter) } {
            new.attach(me);
        }
        self.adapter_changed();
    }

    fn prepare_read(self: Pin<&mut Self>, _string: bool) {
        if !self.prepared {
            if self.should_block_read() {
                self.load_sync();
            } else {
                self.load_async();
            }
        }
    }

    fn emit_data_changed(mut self: Pin<&mut Self>) {
        self.as_mut().internal_data_changed();
        self.as_mut().internal_text_changed();
        self.as_mut().data_changed();
        self.as_mut().text_changed();
        if let Some(a) = unsafe { crate::jsonadapter::Adapter::from_ptr(self.adapter) } {
            a.deserialize(&self.state.data);
        }
    }

    fn update_state(mut self: Pin<&mut Self>, new: State) {
        let path_changed = new.path != self.state.path;
        let data_changed = path_changed || new.data != self.state.data;
        let loaded = !new.path.is_empty() && new.exists;
        let mut rust = self.as_mut().rust_mut();
        rust.prepared = true;
        let loaded_changed = rust.loaded_or_async != loaded;
        rust.loaded_or_async = loaded;
        rust.state = new;
        if path_changed {
            self.as_mut().path_changed();
        }
        if loaded_changed {
            self.as_mut().loaded_or_async_changed();
        }
        if data_changed {
            self.emit_data_changed();
        }
    }

    fn start_op(mut self: Pin<&mut Self>, kind: Kind, path: String, work: impl FnOnce(&AtomicBool) -> State + Send + 'static) {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut rust = self.as_mut().rust_mut();
        rust.next_id += 1;
        let id = rust.next_id;
        let qt = self.qt_thread();
        let c = cancel.clone();
        let handle = std::thread::spawn(move || {
            let state = work(&c);
            let _ = qt.queue(move |this| this.op_finished(id));
            state
        });
        self.as_mut().rust_mut().live = Some(Op { id, kind, path, cancel, handle });
    }

    fn load_async(mut self: Pin<&mut Self>) {
        if self.live.as_ref().is_some_and(|o| o.path == self.target_path) {
            return;
        }
        self.as_mut().cancel_async();
        if self.target_path.is_empty() {
            self.update_state(State::default());
            return;
        }
        let (path, print) = (self.target_path.clone(), self.print_errors);
        self.start_op(Kind::Read, path.clone(), move |cancel| read(&path, print, cancel));
    }

    fn load_sync(mut self: Pin<&mut Self>) {
        if self.target_path.is_empty() {
            self.update_state(State::default());
        } else if !self.as_mut().wait_for_job() {
            let state = read(&self.target_path, self.print_errors, &AtomicBool::new(false));
            self.finish(Kind::Read, state);
        }
    }

    fn finish(mut self: Pin<&mut Self>, kind: Kind, state: State) {
        self.as_mut().rust_mut().write_data = None;
        let error = state.error;
        self.as_mut().update_state(state);
        match (kind, error) {
            (Kind::Read, 0) => self.loaded_signal(),
            (Kind::Read, e) => self.load_failed(e),
            (Kind::Write, 0) => self.saved(),
            (Kind::Write, e) => self.save_failed(e),
        }
    }

    fn op_finished(mut self: Pin<&mut Self>, id: u64) {
        if self.live.as_ref().is_none_or(|o| o.id != id) {
            return;
        }
        let op = self.as_mut().rust_mut().live.take().unwrap();
        let state = op.handle.join().unwrap_or_default();
        self.finish(op.kind, state);
    }

    fn cancel_async(mut self: Pin<&mut Self>) {
        let Some(op) = self.live.as_ref() else { return };
        op.cancel.store(true, Ordering::Release);
        if op.kind == Kind::Read {
            self.as_mut().rust_mut().live = None;
        } else {
            self.wait_for_job();
        }
    }

    fn wait_for_job(mut self: Pin<&mut Self>) -> bool {
        let Some(op) = self.as_mut().rust_mut().live.take() else { return false };
        let state = op.handle.join().unwrap_or_default();
        self.finish(op.kind, state);
        true
    }

    fn reload(self: Pin<&mut Self>) {
        self.update_path();
    }

    fn write_cmp(&self) -> &[u8] {
        self.write_data.as_deref().unwrap_or(&self.state.data)
    }

    fn set_text(self: Pin<&mut Self>, text: &QString) {
        self.set_bytes(text.to_string().into_bytes());
    }

    fn set_data(self: Pin<&mut Self>, data: &QByteArray) {
        self.set_bytes(data.as_slice().to_vec());
    }

    pub fn set_bytes(mut self: Pin<&mut Self>, data: Vec<u8>) {
        if self.write_cmp() == data.as_slice() {
            return;
        }
        if self.target_path.is_empty() {
            eprintln!("FileView: Cannot write file, as no path has been specified.");
            return;
        }
        self.as_mut().rust_mut().write_data = Some(data.clone());
        let (path, atomic, print) = (self.target_path.clone(), self.atomic_writes, self.print_errors);
        if self.block_writes {
            if self.live.is_some() {
                self.as_mut().cancel_async();
            }
            let state = write(&path, &data, atomic, print);
            self.finish(Kind::Write, state);
        } else {
            self.as_mut().cancel_async();
            self.as_mut().rust_mut().write_data = Some(data.clone());
            self.start_op(Kind::Write, path.clone(), move |_| write(&path, &data, atomic, print));
        }
    }

    fn write_adapter(self: Pin<&mut Self>) {
        match unsafe { crate::jsonadapter::Adapter::from_ptr(self.adapter) } {
            Some(a) => {
                let data = a.serialize();
                self.set_bytes(data);
            }
            None => eprintln!("FileView: Cannot call writeAdapter without an adapter."),
        }
    }

    /// Forwarded from the adapter when one of its properties changes.
    pub fn forward_adapter_updated(self: Pin<&mut Self>) {
        self.adapter_updated();
    }

    fn update_watcher(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().watcher = None;
        if self.target_path.is_empty() || !self.watch_changes {
            return;
        }
        let qt = self.qt_thread();
        let sub = crate::watch::subscribe(Path::new(&self.target_path), move || {
            let _ = qt.queue(|this| this.file_changed());
        });
        self.as_mut().rust_mut().watcher = sub;
    }
}

impl Drop for FileViewRust {
    fn drop(&mut self) {
        if let Some(op) = self.live.take() {
            op.cancel.store(true, Ordering::Release);
            if op.kind == Kind::Write {
                let _ = op.handle.join();
            }
        }
    }
}
