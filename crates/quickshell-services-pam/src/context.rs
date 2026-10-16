//! PamContext (upstream services/pam/qml.cpp): one authentication at a time, run by a helper
//! process (see helper.rs). A reader thread queues the helper's events back to the Qt thread;
//! each conversation has a generation so events of an aborted one are dropped.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, active, READ = is_active, WRITE = set_active, NOTIFY = active_changed)]
        #[qproperty(QString, config, READ = config, WRITE = set_config, NOTIFY = config_changed)]
        #[qproperty(QString, configDirectory, READ = config_directory, WRITE = set_config_directory, NOTIFY = config_directory_changed)]
        #[qproperty(QString, user, READ = user, WRITE = set_user, NOTIFY = user_changed)]
        #[qproperty(QString, _qsConfigPath, READ = config_path, WRITE = set_config_path)]
        #[qproperty(QString, message, READ, NOTIFY)]
        #[qproperty(bool, message_is_error, cxx_name = "messageIsError", READ, NOTIFY)]
        #[qproperty(bool, response_required, cxx_name = "responseRequired", READ, NOTIFY)]
        #[qproperty(bool, response_visible, cxx_name = "responseVisible", READ, NOTIFY)]
        type PamContextBase = super::PamContextRust;

        fn is_active(self: &PamContextBase) -> bool;
        fn set_active(self: Pin<&mut PamContextBase>, active: bool);
        #[qsignal]
        #[cxx_name = "activeChanged"]
        fn active_changed(self: Pin<&mut PamContextBase>);
        fn config(self: &PamContextBase) -> QString;
        fn set_config(self: Pin<&mut PamContextBase>, value: QString);
        #[qsignal]
        #[cxx_name = "configChanged"]
        fn config_changed(self: Pin<&mut PamContextBase>);
        fn config_directory(self: &PamContextBase) -> QString;
        fn set_config_directory(self: Pin<&mut PamContextBase>, value: QString);
        #[qsignal]
        #[cxx_name = "configDirectoryChanged"]
        fn config_directory_changed(self: Pin<&mut PamContextBase>);
        fn user(self: &PamContextBase) -> QString;
        fn set_user(self: Pin<&mut PamContextBase>, value: QString);
        #[qsignal]
        #[cxx_name = "userChanged"]
        fn user_changed(self: Pin<&mut PamContextBase>);
        fn config_path(self: &PamContextBase) -> QString;
        fn set_config_path(self: Pin<&mut PamContextBase>, value: QString);

        /// Emitted whenever authentication completes (PamResult value).
        #[qsignal]
        fn completed(self: Pin<&mut PamContextBase>, result: i32);
        /// Emitted when pam fails to authenticate normally (PamError value), before
        /// `completed(PamResult.Error)`.
        #[qsignal]
        fn error(self: Pin<&mut PamContextBase>, error: i32);
        /// A new message, after the change signals of message, messageIsError, responseRequired.
        #[qsignal]
        #[cxx_name = "pamMessage"]
        fn pam_message(self: Pin<&mut PamContextBase>);

        #[qinvokable]
        fn start(self: Pin<&mut PamContextBase>) -> bool;
        #[qinvokable]
        fn abort(self: Pin<&mut PamContextBase>);
        #[qinvokable]
        fn respond(self: Pin<&mut PamContextBase>, response: &QString);
        /// QQmlParserStatus::componentComplete, called by PamContext.qml.
        #[qinvokable]
        #[cxx_name = "_qsComplete"]
        fn complete(self: Pin<&mut PamContextBase>);
    }

    impl cxx_qt::Threading for PamContextBase {}
}

use crate::helper::{self, Event, ExitCode};
use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use std::ffi::{CStr, c_char, c_int};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

const RESULT_SUCCESS: i32 = 0;
const RESULT_FAILED: i32 = 1;
const RESULT_ERROR: i32 = 2;
const RESULT_MAX_TRIES: i32 = 3;
const ERROR_START_FAILED: i32 = 1;
const ERROR_TRY_AUTH_FAILED: i32 = 2;
const ERROR_INTERNAL: i32 = 3;

struct Conversation {
    child: Child,
    stdin: ChildStdin,
}

impl Drop for Conversation {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct PamContextRust {
    post_init: bool,
    target_active: bool,
    config: QString,
    config_directory: QString,
    config_path: QString,
    user: QString,
    message: QString,
    message_is_error: bool,
    response_required: bool,
    response_visible: bool,
    conversation: Option<Conversation>,
    generation: u64,
}

impl Default for PamContextRust {
    fn default() -> Self {
        Self {
            post_init: false,
            target_active: false,
            config: QString::from("login"),
            config_directory: QString::from("/etc/pam.d"),
            config_path: QString::from("/etc/pam.d"),
            user: QString::default(),
            message: QString::default(),
            message_is_error: false,
            response_required: false,
            response_visible: false,
            conversation: None,
            generation: 0,
        }
    }
}

#[repr(C)]
struct Passwd {
    pw_name: *mut c_char,
    pw_passwd: *mut c_char,
    pw_uid: u32,
    pw_gid: u32,
    pw_gecos: *mut c_char,
    pw_dir: *mut c_char,
    pw_shell: *mut c_char,
}

unsafe extern "C" {
    fn getuid() -> u32;
    fn getpwuid_r(uid: u32, pwd: *mut Passwd, buf: *mut c_char, len: usize, result: *mut *mut Passwd) -> c_int;
    fn getpwnam_r(name: *const c_char, pwd: *mut Passwd, buf: *mut c_char, len: usize, result: *mut *mut Passwd) -> c_int;
}

/// The account name for `user`, or the current user when empty (upstream's getpw*_r checks).
fn resolve_user(user: &str) -> Result<String, String> {
    let mut buf = vec![0 as c_char; 16384];
    let mut pwd: Passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut Passwd = std::ptr::null_mut();
    let r = if user.is_empty() {
        unsafe { getpwuid_r(getuid(), &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) }
    } else {
        let name = std::ffi::CString::new(user).map_err(|_| format!("invalid user name {user:?}"))?;
        unsafe { getpwnam_r(name.as_ptr(), &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) }
    };
    if result.is_null() {
        return Err(if r == 0 && !user.is_empty() {
            format!("specified user was not found: {user}")
        } else {
            format!("error in getpw*_r: {r}")
        });
    }
    Ok(unsafe { CStr::from_ptr(pwd.pw_name) }.to_string_lossy().into_owned())
}

fn spawn_helper(dir: &str, config: &str, user: &str) -> std::io::Result<Child> {
    Command::new(std::env::current_exe()?)
        .arg(helper::HELPER_ARG)
        .args([dir, config, user])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
}

impl qobject::PamContextBase {
    fn is_active(&self) -> bool {
        self.conversation.is_some()
    }

    fn set_active(mut self: Pin<&mut Self>, active: bool) {
        if active == self.target_active {
            return;
        }
        self.as_mut().rust_mut().target_active = active;
        if active {
            self.start_conversation();
        } else {
            self.abort_conversation();
        }
    }

    fn config(&self) -> QString {
        self.config.clone()
    }

    fn config_directory(&self) -> QString {
        self.config_directory.clone()
    }

    fn user(&self) -> QString {
        self.user.clone()
    }

    fn config_path(&self) -> QString {
        self.config_path.clone()
    }

    fn set_config_path(mut self: Pin<&mut Self>, value: QString) {
        self.as_mut().rust_mut().config_path = value;
    }

    /// Shared guard of the config/configDirectory/user setters.
    fn settable(&self, name: &str) -> bool {
        if self.is_active() {
            quickshell::qs_log!(warn, "quickshell.service.pam", "Cannot set {name} on PamContext while it is active.");
            return false;
        }
        true
    }

    fn set_config(mut self: Pin<&mut Self>, value: QString) {
        if value == self.config || !self.settable("config") {
            return;
        }
        self.as_mut().rust_mut().config = value;
        self.config_changed();
    }

    fn set_config_directory(mut self: Pin<&mut Self>, value: QString) {
        if value == self.config_directory || !self.settable("configDirectory") {
            return;
        }
        self.as_mut().rust_mut().config_directory = value;
        self.config_directory_changed();
    }

    fn set_user(mut self: Pin<&mut Self>, value: QString) {
        if value == self.user || !self.settable("user") {
            return;
        }
        self.as_mut().rust_mut().user = value;
        self.user_changed();
    }

    fn start(mut self: Pin<&mut Self>) -> bool {
        self.as_mut().set_active(true);
        self.is_active()
    }

    fn abort(self: Pin<&mut Self>) {
        self.set_active(false);
    }

    fn respond(mut self: Pin<&mut Self>, response: &QString) {
        if !(self.is_active() && self.response_required) {
            quickshell::qs_log!(warn, "quickshell.service.pam", "PamContext response was ignored as this context does not require one.");
            return;
        }
        let line = format!("{}\n", helper::escape(&response.to_string()));
        let ok = self
            .as_mut()
            .rust_mut()
            .conversation
            .as_mut()
            .is_some_and(|c| c.stdin.write_all(line.as_bytes()).and_then(|_| c.stdin.flush()).is_ok());
        if !ok {
            quickshell::qs_log!(warn, "quickshell.service.pam", "pam: failed to write response to subprocess.");
            self.fail(ERROR_INTERNAL);
        }
    }

    fn complete(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().post_init = true;
        if self.target_active {
            self.start_conversation();
        }
    }

    fn fail_start(mut self: Pin<&mut Self>, why: String) {
        quickshell::qs_log!(warn, "quickshell.service.pam", "Cannot start PamContext because {why}");
        self.as_mut().rust_mut().target_active = false;
    }

    fn start_conversation(mut self: Pin<&mut Self>) {
        if !self.post_init || self.conversation.is_some() {
            return;
        }
        let dir = self.config_path.to_string();
        let config = self.config.to_string();
        if !Path::new(&dir).is_dir() {
            return self.fail_start(format!("specified config directory {dir:?} is not a directory."));
        }
        let file = Path::new(&dir).join(&config);
        if !file.is_file() {
            return self.fail_start(format!("specified config file {:?} is not a file.", file.display()));
        }
        let user = match resolve_user(&self.user.to_string()) {
            Ok(u) => u,
            Err(e) => return self.fail_start(e),
        };
        let mut child = match spawn_helper(&dir, &config, &user) {
            Ok(c) => c,
            Err(e) => {
                quickshell::qs_log!(warn, "quickshell.service.pam", "pam: failed to create subprocess: {e}");
                self.as_mut().rust_mut().target_active = false;
                self.as_mut().active_changed();
                self.as_mut().error(ERROR_INTERNAL);
                self.completed(RESULT_ERROR);
                return;
            }
        };
        let (stdin, stdout) = (child.stdin.take().unwrap(), child.stdout.take().unwrap());
        let generation = self.generation + 1;
        {
            let mut rust = self.as_mut().rust_mut();
            rust.generation = generation;
            rust.conversation = Some(Conversation { child, stdin });
        }
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let event = helper::parse_event(&line);
                let exit = matches!(event, Some(Event::Exit(_)) | None);
                let _ = qt.queue(move |obj| obj.on_event(generation, event));
                if exit {
                    return;
                }
            }
            // Pipe closed without an exit event: the helper died.
            let _ = qt.queue(move |obj| obj.on_event(generation, None));
        });
        self.active_changed();
    }

    fn abort_conversation(mut self: Pin<&mut Self>) {
        if self.conversation.is_none() {
            return;
        }
        {
            let mut rust = self.as_mut().rust_mut();
            rust.target_active = false;
            rust.conversation = None; // kills and reaps the helper
        }
        self.as_mut().active_changed();
        if !self.message.is_empty() {
            self.as_mut().rust_mut().message = QString::default();
            self.as_mut().message_changed();
        }
        if self.message_is_error {
            self.as_mut().rust_mut().message_is_error = false;
            self.as_mut().message_is_error_changed();
        }
        if self.response_required {
            self.as_mut().rust_mut().response_required = false;
            self.as_mut().response_required_changed();
        }
    }

    fn finish(mut self: Pin<&mut Self>, result: i32) {
        self.as_mut().abort_conversation();
        self.completed(result);
    }

    fn fail(mut self: Pin<&mut Self>, error: i32) {
        self.as_mut().abort_conversation();
        self.as_mut().error(error);
        self.completed(RESULT_ERROR);
    }

    fn on_event(mut self: Pin<&mut Self>, generation: u64, event: Option<Event>) {
        if generation != self.generation || self.conversation.is_none() {
            return;
        }
        match event {
            Some(Event::Exit(code)) => match code {
                ExitCode::Success => self.finish(RESULT_SUCCESS),
                ExitCode::AuthFailed => self.finish(RESULT_FAILED),
                ExitCode::MaxTries => self.finish(RESULT_MAX_TRIES),
                ExitCode::StartFailed => self.fail(ERROR_START_FAILED),
                ExitCode::PamError => self.fail(ERROR_TRY_AUTH_FAILED),
                ExitCode::OtherError => self.fail(ERROR_INTERNAL),
            },
            Some(Event::Message { text, error, required, echo }) => {
                let text = QString::from(&text);
                if text != self.message {
                    self.as_mut().rust_mut().message = text;
                    self.as_mut().message_changed();
                }
                if error != self.message_is_error {
                    self.as_mut().rust_mut().message_is_error = error;
                    self.as_mut().message_is_error_changed();
                }
                if echo != self.response_visible {
                    self.as_mut().rust_mut().response_visible = echo;
                    self.as_mut().response_visible_changed();
                }
                if required != self.response_required {
                    self.as_mut().rust_mut().response_required = required;
                    self.as_mut().response_required_changed();
                }
                self.pam_message();
            }
            None => {
                quickshell::qs_log!(warn, "quickshell.service.pam", "pam: failed to read subprocess request.");
                self.fail(ERROR_INTERNAL);
            }
        }
    }
}
