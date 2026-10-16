//! One authentication conversation with polkit's privileged helper, the part upstream gets from
//! libpolkit-agent-1's PolkitAgentSession (polkit 127 polkitagentsession.c). The helper runs PAM
//! as root and reports the result to polkitd itself; the agent only relays the conversation.
//!
//! Wire protocol: the systemd socket (`Accept=yes`, helper started with --socket-activated) takes
//! "user\ncookie\n"; the setuid fallback takes the user as argv[1] and "cookie\n" on stdin. The
//! helper answers one line per PAM message, `PREFIX g_strescape(message)`, then SUCCESS or
//! FAILURE. Responses go back raw, newline terminated. Cancel closes the socket or SIGTERMs the
//! helper.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};

const SOCKET: &str = "/run/polkit/agent-helper.socket";
const HELPER: &str = "/usr/lib/polkit-1/polkit-agent-helper-1";

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Request { prompt: String, echo: bool },
    Error(String),
    Info(String),
    Completed(bool),
}

enum Handle {
    Socket(UnixStream),
    Child(i32),
}

pub struct Session {
    writer: Box<dyn Write + Send>,
    handle: Handle,
}

impl Session {
    /// Starts the conversation for `user`; `emit` runs on a reader thread for every event and
    /// ends with exactly one `Completed`.
    pub fn start(user: &str, cookie: &str, emit: impl Fn(Event) + Send + 'static) -> io::Result<Self> {
        // QSRS_POLKIT_HELPER_SOCKET lets tests serve a fake helper.
        let socket = std::env::var("QSRS_POLKIT_HELPER_SOCKET").unwrap_or_else(|_| SOCKET.into());
        if std::path::Path::new(&socket).exists() {
            match UnixStream::connect(&socket) {
                Ok(stream) => {
                    let mut writer = stream.try_clone()?;
                    writer.write_all(format!("{user}\n{cookie}\n").as_bytes())?;
                    let reader = stream.try_clone()?;
                    std::thread::Builder::new().name("qs-polkit".into()).spawn(move || read(reader, &emit))?;
                    return Ok(Self { writer: Box::new(writer), handle: Handle::Socket(stream) });
                }
                Err(e) => quickshell::qs_log!(
                    warn,
                    "quickshell.service.polkit",
                    "Cannot connect to helper via {socket}, falling back to spawn suid helper instead: {e}"
                ),
            }
        }
        let mut child = Command::new(HELPER).arg(user).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        stdin.write_all(format!("{cookie}\n").as_bytes())?;
        let pid = child.id() as i32;
        std::thread::Builder::new().name("qs-polkit".into()).spawn(move || {
            read(stdout, &emit);
            let _ = child.wait();
        })?;
        Ok(Self { writer: Box::new(stdin), handle: Handle::Child(pid) })
    }

    pub fn respond(&mut self, response: &str) {
        let mut line = response.to_owned();
        if !line.ends_with('\n') {
            line.push('\n');
        }
        let _ = self.writer.write_all(line.as_bytes()).and_then(|_| self.writer.flush());
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        match &self.handle {
            Handle::Socket(s) => {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
            // The reader thread reaps it once its stdout closes.
            Handle::Child(pid) => unsafe {
                libc::kill(*pid, libc::SIGTERM);
            },
        }
    }
}

fn read(stream: impl Read, emit: &dyn Fn(Event)) {
    let mut reader = BufReader::new(stream);
    let mut raw = Vec::new();
    loop {
        raw.clear();
        match reader.read_until(b'\n', &mut raw) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if raw.last() == Some(&b'\n') {
            raw.pop();
        }
        let line = compress(&raw);
        match parse(&line) {
            Some(Event::Completed(ok)) => return emit(Event::Completed(ok)),
            Some(event) => emit(event),
            None => {
                quickshell::qs_log!(warn, "quickshell.service.polkit", "Unknown line '{line}' from helper");
                break;
            }
        }
    }
    emit(Event::Completed(false));
}

/// polkitagentsession.c io_watch_have_data: prefixes are matched after unescaping the line.
pub fn parse(line: &str) -> Option<Event> {
    let event = if let Some(m) = line.strip_prefix("PAM_PROMPT_ECHO_OFF ") {
        Event::Request { prompt: m.into(), echo: false }
    } else if let Some(m) = line.strip_prefix("PAM_PROMPT_ECHO_ON ") {
        Event::Request { prompt: m.into(), echo: true }
    } else if let Some(m) = line.strip_prefix("PAM_ERROR_MSG ") {
        Event::Error(m.into())
    } else if let Some(m) = line.strip_prefix("PAM_TEXT_INFO ") {
        Event::Info(m.into())
    } else if line.starts_with("SUCCESS") {
        Event::Completed(true)
    } else if line.starts_with("FAILURE") {
        Event::Completed(false)
    } else {
        return None;
    };
    Some(event)
}

/// g_strcompress: C escapes and up to three octal digits (g_strescape writes non-ASCII bytes
/// that way, so UTF-8 is rebuilt from the bytes).
pub fn compress(s: &[u8]) -> String {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'\\' {
            out.push(s[i]);
            i += 1;
            continue;
        }
        i += 1;
        let Some(&c) = s.get(i) else { break };
        if (b'0'..=b'7').contains(&c) {
            let mut v: u32 = 0;
            let mut n = 0;
            while n < 3 && i < s.len() && (b'0'..=b'7').contains(&s[i]) {
                v = v * 8 + u32::from(s[i] - b'0');
                i += 1;
                n += 1;
            }
            out.push(v as u8);
            continue;
        }
        out.push(match c {
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            other => other,
        });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compresses_like_glib() {
        assert_eq!(compress(br"Contrase\303\261a: "), "Contraseña: ");
        assert_eq!(compress(br#"a\\b\"c\td\n"#), "a\\b\"c\td\n");
        assert_eq!(compress(br"x\101y"), "xAy");
        assert_eq!(compress(b"plain"), "plain");
    }

    #[test]
    fn parses_helper_lines() {
        assert_eq!(parse("PAM_PROMPT_ECHO_OFF Password: "), Some(Event::Request { prompt: "Password: ".into(), echo: false }));
        assert_eq!(parse("PAM_PROMPT_ECHO_ON Login: "), Some(Event::Request { prompt: "Login: ".into(), echo: true }));
        assert_eq!(parse("PAM_ERROR_MSG nope"), Some(Event::Error("nope".into())));
        assert_eq!(parse("PAM_TEXT_INFO hi"), Some(Event::Info("hi".into())));
        assert_eq!(parse("SUCCESS"), Some(Event::Completed(true)));
        assert_eq!(parse("FAILURE"), Some(Event::Completed(false)));
        assert_eq!(parse("garbage"), None);
    }
}
