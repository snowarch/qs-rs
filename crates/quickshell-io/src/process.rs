//! `Process`: runs a child process off the GUI thread and feeds its output to stream parsers.
//! Reader and waiter threads queue results back to the Qt thread; a run id drops events from
//! a previous run after a restart.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qhash.h");
        type QHash_QString_QVariant = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, running, READ, WRITE = set_running, NOTIFY)]
        #[qproperty(QVariant, process_id, cxx_name = "processId", READ, NOTIFY = process_id_changed)]
        #[qproperty(QStringList, command, READ, WRITE = set_command, NOTIFY)]
        #[qproperty(QString, working_directory, cxx_name = "workingDirectory")]
        #[qproperty(QHash_QString_QVariant, environment)]
        #[qproperty(bool, clear_environment, cxx_name = "clearEnvironment")]
        #[qproperty(*mut QObject, stdout, READ, WRITE = set_stdout, NOTIFY = stdout_changed)]
        #[qproperty(*mut QObject, stderr, READ, WRITE = set_stderr, NOTIFY = stderr_changed)]
        #[qproperty(bool, stdin_enabled, cxx_name = "stdinEnabled", READ, WRITE = set_stdin_enabled, NOTIFY)]
        type Process = super::ProcessRust;

        fn set_running(self: Pin<&mut Process>, running: bool);
        fn set_command(self: Pin<&mut Process>, command: QStringList);
        unsafe fn set_stdout(self: Pin<&mut Process>, parser: *mut QObject);
        unsafe fn set_stderr(self: Pin<&mut Process>, parser: *mut QObject);
        fn set_stdin_enabled(self: Pin<&mut Process>, enabled: bool);

        #[qsignal]
        #[cxx_name = "processIdChanged"]
        fn process_id_changed(self: Pin<&mut Process>);
        #[qsignal]
        #[cxx_name = "stdoutParserChanged"]
        fn stdout_changed(self: Pin<&mut Process>);
        #[qsignal]
        #[cxx_name = "stderrParserChanged"]
        fn stderr_changed(self: Pin<&mut Process>);
        #[qsignal]
        fn started(self: Pin<&mut Process>);
        #[qsignal]
        fn exited(self: Pin<&mut Process>, #[cxx_name = "exitCode"] exit_code: i32, #[cxx_name = "exitStatus"] exit_status: i32);

        #[qinvokable]
        fn exec(self: Pin<&mut Process>, context: &QVariant);
        #[qinvokable]
        fn signal(self: Pin<&mut Process>, signal: i32);
        #[qinvokable]
        fn write(self: Pin<&mut Process>, data: &QString);
        #[qinvokable]
        #[cxx_name = "startDetached"]
        fn start_detached(self: Pin<&mut Process>);
    }

    impl cxx_qt::Threading for Process {}
}

use crate::datastream::Parser;
use quickshell::proc::{Context, Event, Spawned};
use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QHash, QHashPair_QString_QVariant, QString, QStringList, QVariant};

pub struct ProcessRust {
    running: bool,
    process_id: QVariant,
    command: QStringList,
    working_directory: QString,
    environment: QHash<QHashPair_QString_QVariant>,
    clear_environment: bool,
    stdout: *mut cxx_qt::QObject,
    stderr: *mut cxx_qt::QObject,
    stdin_enabled: bool,

    target_running: bool,
    start_queued: bool,
    run_id: u64,
    pid: Option<u32>,
    stdin: Option<quickshell::proc::Stdin>,
    stdout_buf: Vec<u8>,
    stderr_buf: Vec<u8>,
}

impl Default for ProcessRust {
    fn default() -> Self {
        Self {
            running: false,
            process_id: quickshell::util::null_variant(),
            command: QStringList::default(),
            working_directory: QString::default(),
            environment: QHash::default(),
            clear_environment: false,
            stdout: std::ptr::null_mut(),
            stderr: std::ptr::null_mut(),
            stdin_enabled: false,
            target_running: false,
            start_queued: false,
            run_id: 0,
            pid: None,
            stdin: None,
            stdout_buf: Vec::new(),
            stderr_buf: Vec::new(),
        }
    }
}

impl Drop for ProcessRust {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            quickshell::proc::kill(pid, libc::SIGKILL);
        }
    }
}

impl qobject::Process {
    fn set_running(mut self: Pin<&mut Self>, running: bool) {
        self.as_mut().rust_mut().target_running = running;
        if running {
            self.queue_start();
        } else if let Some(pid) = self.pid {
            quickshell::proc::kill(pid, libc::SIGTERM);
        }
    }

    fn set_command(mut self: Pin<&mut Self>, command: QStringList) {
        if self.command == command {
            return;
        }
        self.as_mut().rust_mut().command = command;
        self.as_mut().command_changed();
        self.queue_start();
    }

    unsafe fn set_stdout(mut self: Pin<&mut Self>, parser: *mut cxx_qt::QObject) {
        if self.stdout == parser {
            return;
        }
        let mut rust = self.as_mut().rust_mut();
        rust.stdout = parser;
        let mut buf = std::mem::take(&mut rust.stdout_buf);
        self.as_mut().stdout_changed();
        if !buf.is_empty() {
            if let Some(p) = unsafe { Parser::from_ptr(parser) } {
                p.feed(&mut buf, &[]);
            }
        }
        self.as_mut().rust_mut().stdout_buf = buf;
    }

    unsafe fn set_stderr(mut self: Pin<&mut Self>, parser: *mut cxx_qt::QObject) {
        if self.stderr == parser {
            return;
        }
        self.as_mut().rust_mut().stderr = parser;
        self.as_mut().stderr_changed();
    }

    fn set_stdin_enabled(mut self: Pin<&mut Self>, enabled: bool) {
        if self.stdin_enabled == enabled {
            return;
        }
        let mut rust = self.as_mut().rust_mut();
        rust.stdin_enabled = enabled;
        if !enabled {
            rust.stdin = None;
        }
        self.as_mut().stdin_enabled_changed();
    }

    /// Starts on the next event loop turn so every binding of a fresh object is applied first.
    fn queue_start(mut self: Pin<&mut Self>) {
        if self.start_queued {
            return;
        }
        self.as_mut().rust_mut().start_queued = true;
        let _ = self.qt_thread().queue(|mut this| {
            this.as_mut().rust_mut().start_queued = false;
            this.start_if_ready();
        });
    }

    fn context(&self) -> Context {
        Context {
            command: self.command.iter().map(|s| s.to_string()).collect(),
            working_directory: self.working_directory.to_string(),
            environment: quickshell::proc::env_from_hash(&self.environment),
            clear_environment: self.clear_environment,
        }
    }

    fn start_if_ready(mut self: Pin<&mut Self>) {
        if self.pid.is_some() || !self.target_running || self.command.len() == 0 {
            return;
        }
        let ctx = self.context();
        let mut rust = self.as_mut().rust_mut();
        rust.target_running = false;
        rust.run_id += 1;
        rust.stdout_buf.clear();
        rust.stderr_buf.clear();
        let run_id = rust.run_id;
        let (want_out, want_err, want_in) = (!rust.stdout.is_null(), !rust.stderr.is_null(), rust.stdin_enabled);
        let qt = self.qt_thread();
        let spawned = quickshell::proc::spawn(&ctx, want_out, want_err, want_in, move |event| {
            let _ = qt.queue(move |this| this.on_event(run_id, event));
        });
        match spawned {
            Ok(Spawned { pid, stdin }) => {
                let mut rust = self.as_mut().rust_mut();
                rust.pid = Some(pid);
                rust.stdin = stdin;
                rust.running = true;
                rust.process_id = QVariant::from(&(pid as i64));
                self.as_mut().process_id_changed();
                self.as_mut().running_changed();
                self.as_mut().started();
            }
            Err(e) => {
                quickshell::qs_log!(warn, "quickshell.io.process", "Process failed to start, likely because the binary could not be found. Command: {:?}: {e}", ctx.command);
            }
        }
    }

    fn on_event(mut self: Pin<&mut Self>, run_id: u64, event: Event) {
        if run_id != self.run_id {
            return;
        }
        match event {
            Event::Stdout(bytes) => {
                let mut buf = std::mem::take(&mut self.as_mut().rust_mut().stdout_buf);
                if let Some(p) = unsafe { Parser::from_ptr(self.stdout) } {
                    p.feed(&mut buf, &bytes);
                }
                self.as_mut().rust_mut().stdout_buf = buf;
            }
            Event::Stderr(bytes) => {
                let mut buf = std::mem::take(&mut self.as_mut().rust_mut().stderr_buf);
                if let Some(p) = unsafe { Parser::from_ptr(self.stderr) } {
                    p.feed(&mut buf, &bytes);
                }
                self.as_mut().rust_mut().stderr_buf = buf;
            }
            Event::Exited { code, crashed } => {
                let mut rust = self.as_mut().rust_mut();
                rust.pid = None;
                rust.stdin = None;
                rust.running = false;
                rust.process_id = quickshell::util::null_variant();
                let mut out = std::mem::take(&mut rust.stdout_buf);
                let mut err = std::mem::take(&mut rust.stderr_buf);
                if let Some(p) = unsafe { Parser::from_ptr(self.stdout) } {
                    p.end(&mut out);
                }
                if let Some(p) = unsafe { Parser::from_ptr(self.stderr) } {
                    p.end(&mut err);
                }
                self.as_mut().exited(code, crashed as i32);
                self.as_mut().running_changed();
                self.as_mut().process_id_changed();
                self.start_if_ready();
            }
        }
    }

    fn exec(mut self: Pin<&mut Self>, context: &QVariant) {
        self.as_mut().set_running(false);
        let ctx = quickshell::proc::context_from_variant(context);
        if let Some(cmd) = ctx.command {
            let list: QStringList = cmd.iter().map(|s| QString::from(s.as_str())).collect();
            self.as_mut().set_command(list);
        }
        if let Some(env) = ctx.environment {
            self.as_mut().set_environment(env);
        }
        if let Some(clear) = ctx.clear_environment {
            self.as_mut().set_clear_environment(clear);
        }
        if let Some(dir) = ctx.working_directory {
            self.as_mut().set_working_directory(QString::from(dir.as_str()));
        }
        if self.command.len() == 0 {
            quickshell::qs_log!(warn, "quickshell.io.process", "Cannot start process as command is empty.");
            return;
        }
        self.set_running(true);
    }

    fn signal(self: Pin<&mut Self>, signal: i32) {
        if let Some(pid) = self.pid {
            quickshell::proc::kill(pid, signal);
        }
    }

    fn write(self: Pin<&mut Self>, data: &QString) {
        if let Some(tx) = &self.stdin {
            let _ = tx.send(data.to_string().into_bytes());
        }
    }

    fn start_detached(self: Pin<&mut Self>) {
        if self.command.len() == 0 {
            quickshell::qs_log!(warn, "quickshell.io.process", "Cannot start process as command is empty.");
            return;
        }
        quickshell::proc::spawn_detached(&self.context());
    }
}
