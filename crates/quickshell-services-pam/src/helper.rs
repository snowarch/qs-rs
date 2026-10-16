//! The PAM conversation runs in a helper process (qs-rs re-executed with `--qs-pam-helper`), like
//! upstream's forked subprocess: a module blocked in pam_authenticate (fingerprint readers) can
//! only be stopped by killing it. Protocol over the helper's stdin/stdout, one line per event:
//!   helper -> shell: `M<error><required><echo>\t<text>` (flags '0'/'1'), then `X<code>` at the end
//!   shell -> helper: the response text
//! Text is escaped (`\\` and `\n`) so it stays on one line.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io::{BufRead, Write};

pub const HELPER_ARG: &str = "--qs-pam-helper";

/// Exit codes, same order as upstream's PamIpcExitCode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitCode {
    Success = 0,
    StartFailed = 1,
    AuthFailed = 2,
    MaxTries = 3,
    PamError = 4,
    OtherError = 5,
}

impl ExitCode {
    pub fn from_digit(c: u8) -> Self {
        match c {
            b'0' => Self::Success,
            b'1' => Self::StartFailed,
            b'2' => Self::AuthFailed,
            b'3' => Self::MaxTries,
            b'4' => Self::PamError,
            _ => Self::OtherError,
        }
    }
}

pub fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Message { text: String, error: bool, required: bool, echo: bool },
    Exit(ExitCode),
}

/// Parses one helper line; None for anything malformed.
pub fn parse_event(line: &str) -> Option<Event> {
    let b = line.as_bytes();
    match b.first()? {
        b'X' => Some(Event::Exit(ExitCode::from_digit(*b.get(1)?))),
        b'M' if b.len() >= 5 && b[4] == b'\t' => Some(Event::Message {
            error: b[1] == b'1',
            required: b[2] == b'1',
            echo: b[3] == b'1',
            text: unescape(&line[5..]),
        }),
        _ => None,
    }
}

const PAM_SUCCESS: c_int = 0;
const PAM_AUTH_ERR: c_int = 7;
const PAM_MAXTRIES: c_int = 11;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_ERROR_MSG: c_int = 3;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type ConvFn = extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int;

#[repr(C)]
struct PamConv {
    conv: ConvFn,
    appdata_ptr: *mut c_void,
}

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start_confdir(
        service: *const c_char,
        user: *const c_char,
        conv: *const PamConv,
        confdir: *const c_char,
        pamh: *mut *mut c_void,
    ) -> c_int;
    fn pam_authenticate(pamh: *mut c_void, flags: c_int) -> c_int;
    fn pam_end(pamh: *mut c_void, status: c_int) -> c_int;
    fn pam_strerror(pamh: *mut c_void, errnum: c_int) -> *const c_char;
}

unsafe extern "C" {
    fn calloc(n: usize, size: usize) -> *mut c_void;
    fn strdup(s: *const c_char) -> *mut c_char;
    fn free(p: *mut c_void);
}

fn send(line: &str) -> bool {
    let mut out = std::io::stdout().lock();
    out.write_all(line.as_bytes()).and_then(|_| out.write_all(b"\n")).and_then(|_| out.flush()).is_ok()
}

fn strerror(handle: *mut c_void, code: c_int) -> String {
    let p = unsafe { pam_strerror(handle, code) };
    if p.is_null() { format!("code {code}") } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
}

/// Relays each pam message to the shell and waits for a response when one is required. Any
/// broken pipe ends the helper, like upstream's `_exit`.
extern "C" fn conversation(
    count: c_int,
    messages: *mut *const PamMessage,
    responses: *mut *mut PamResponse,
    _appdata: *mut c_void,
) -> c_int {
    let count = count.max(0) as usize;
    // freed by libpam, so it must come from the C allocator.
    let resp = unsafe { calloc(count, std::mem::size_of::<PamResponse>()) } as *mut PamResponse;
    let stdin = std::io::stdin();
    for i in 0..count {
        let msg = unsafe { &**messages.add(i) };
        let text = if msg.msg.is_null() { String::new() } else { unsafe { CStr::from_ptr(msg.msg) }.to_string_lossy().into_owned() };
        let echo = msg.msg_style != PAM_PROMPT_ECHO_OFF;
        let error = msg.msg_style == PAM_ERROR_MSG;
        let required = msg.msg_style == PAM_PROMPT_ECHO_OFF || msg.msg_style == PAM_PROMPT_ECHO_ON;
        let flag = |b: bool| if b { '1' } else { '0' };
        if !send(&format!("M{}{}{}\t{}", flag(error), flag(required), flag(echo), escape(&text))) {
            unsafe { free(resp as *mut c_void) };
            std::process::exit(1);
        }
        if required {
            let mut line = String::new();
            if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
                std::process::exit(ExitCode::OtherError as i32);
            }
            let answer = unescape(line.strip_suffix('\n').unwrap_or(&line));
            let c = CString::new(answer).unwrap_or_default();
            unsafe { (*resp.add(i)).resp = strdup(c.as_ptr()) };
        }
    }
    unsafe { *responses = resp };
    PAM_SUCCESS
}

fn authenticate(config_dir: &str, config: &str, user: &str) -> ExitCode {
    let (Ok(dir), Ok(service), Ok(user)) = (CString::new(config_dir), CString::new(config), CString::new(user)) else {
        return ExitCode::OtherError;
    };
    let conv = PamConv { conv: conversation, appdata_ptr: std::ptr::null_mut() };
    let mut handle: *mut c_void = std::ptr::null_mut();
    let r = unsafe { pam_start_confdir(service.as_ptr(), user.as_ptr(), &conv, dir.as_ptr(), &mut handle) };
    if r != PAM_SUCCESS {
        eprintln!("pam: unable to start conversation: {}", strerror(handle, r));
        return ExitCode::StartFailed;
    }
    let r = unsafe { pam_authenticate(handle, 0) };
    let code = match r {
        PAM_SUCCESS => ExitCode::Success,
        PAM_AUTH_ERR => ExitCode::AuthFailed,
        PAM_MAXTRIES => ExitCode::MaxTries,
        _ => {
            eprintln!("pam: error while authenticating: {}", strerror(handle, r));
            ExitCode::PamError
        }
    };
    let e = unsafe { pam_end(handle, r) };
    if e != PAM_SUCCESS {
        eprintln!("pam: error in pam_end: {}", strerror(handle, e));
    }
    code
}

/// Entry point of the helper process: `qs-rs --qs-pam-helper CONFIG_DIR CONFIG USER`.
pub fn main(args: &[String]) -> i32 {
    let [dir, config, user] = args else { return 2 };
    let code = authenticate(dir, config, user);
    if send(&format!("X{}", code as i32)) { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_round_trip() {
        for s in ["", "Password: ", "a\nb", "back\\slash\\n", "tab\tok", "\\"] {
            assert_eq!(unescape(&escape(s)), s);
            assert!(!escape(s).contains('\n'));
        }
    }

    #[test]
    fn parses_events() {
        assert_eq!(parse_event("X0"), Some(Event::Exit(ExitCode::Success)));
        assert_eq!(parse_event("X3"), Some(Event::Exit(ExitCode::MaxTries)));
        assert_eq!(parse_event("X9"), Some(Event::Exit(ExitCode::OtherError)));
        assert_eq!(
            parse_event("M010\tPassword:\\n"),
            Some(Event::Message { text: "Password:\n".into(), error: false, required: true, echo: false })
        );
        assert_eq!(parse_event("M01"), None);
        assert_eq!(parse_event(""), None);
        assert_eq!(parse_event("X"), None);
    }
}
