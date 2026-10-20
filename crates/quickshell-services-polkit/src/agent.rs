//! PolkitAgent (upstream services/polkit: qml.cpp, agentimpl.cpp, flow.cpp, listener.cpp) without
//! libpolkit-agent-1: the agent object is served on the shared system bus connection, registered
//! with polkitd for this process's logind session, and each request runs its conversation through
//! helper.rs. Requests queue and run one at a time; the active one is mirrored into the AuthFlow
//! object that PolkitAgent.qml builds from `flowStarted`.
//!
//! Differences from upstream, on purpose: only polkitd may call the agent; it registers again when
//! polkitd restarts; changing the selected identity does not count as a failed attempt; a request
//! without usable identities does not stall the queue.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, path, READ, WRITE = set_path)]
        #[qproperty(bool, is_registered, cxx_name = "isRegistered", READ, NOTIFY)]
        type PolkitAgentBase = super::PolkitAgentRust;

        fn set_path(self: Pin<&mut PolkitAgentBase>, path: QString);

        /// A request became active: JSON {actionId, message, iconName, cookie, identities:
        /// [{id, string, displayName, isGroup}]}.
        #[qsignal]
        #[cxx_name = "qsFlowStarted"]
        fn flow_started(self: Pin<&mut PolkitAgentBase>, json: QString);
        #[qsignal]
        #[cxx_name = "qsPrompt"]
        fn prompt(self: Pin<&mut PolkitAgentBase>, prompt: QString, echo: bool);
        #[qsignal]
        #[cxx_name = "qsSupplementary"]
        fn supplementary(self: Pin<&mut PolkitAgentBase>, message: QString, error: bool);
        /// A wrong response; a new conversation for the same identity starts right after.
        #[qsignal]
        #[cxx_name = "qsFailed"]
        fn failed(self: Pin<&mut PolkitAgentBase>);
        /// The request ended: authorized, cancelled by the user, or cancelled by polkitd.
        #[qsignal]
        #[cxx_name = "qsFinished"]
        fn finished(self: Pin<&mut PolkitAgentBase>, success: bool, by_agent: bool);
        #[qsignal]
        #[cxx_name = "qsFlowEnded"]
        fn flow_ended(self: Pin<&mut PolkitAgentBase>);

        #[qinvokable]
        #[cxx_name = "_qsComplete"]
        fn complete(self: Pin<&mut PolkitAgentBase>);
        #[qinvokable]
        #[cxx_name = "_qsSubmit"]
        fn submit(self: Pin<&mut PolkitAgentBase>, response: &QString);
        #[qinvokable]
        #[cxx_name = "_qsCancel"]
        fn cancel(self: Pin<&mut PolkitAgentBase>);
        #[qinvokable]
        #[cxx_name = "_qsSelectIdentity"]
        fn select_identity(self: Pin<&mut PolkitAgentBase>, index: i32);
    }

    impl cxx_qt::Threading for PolkitAgentBase {}
}

use crate::helper::{Event, Session};
use core::pin::Pin;
use cxx_qt::{CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::QString;
use qs_dbus::rt;
use zbus::zvariant::{OwnedValue, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};

const LOG: &str = "quickshell.service.polkit";
const POLKITD: &str = "org.freedesktop.PolicyKit1";
const AUTHORITY: &str = "/org/freedesktop/PolicyKit1/Authority";
const AUTHORITY_IFACE: &str = "org.freedesktop.PolicyKit1.Authority";
const LOGIND: &str = "org.freedesktop.login1";

/// polkitd accepts one agent per session, so like upstream's PolkitAgentImpl::instance only the
/// first PolkitAgent of the process registers.
static TAKEN: AtomicBool = AtomicBool::new(false);

type Qt = CxxQtThread<qobject::PolkitAgentBase>;
/// The registration is one per process and outlives a reload: the PolkitAgent of the current
/// generation (token, thread) gets the requests. None while none is attached.
static TARGET: std::sync::Mutex<Option<(u64, Qt)>> = std::sync::Mutex::new(None);
static REGISTERED: AtomicBool = AtomicBool::new(false);
static SERVING: std::sync::Once = std::sync::Once::new();

fn target() -> Option<Qt> {
    TARGET.lock().unwrap().as_ref().map(|(_, qt)| qt.clone())
}

#[derive(Clone)]
struct Identity {
    id: u32,
    name: String,
    display_name: String,
    group: bool,
}

struct Request {
    action_id: String,
    message: String,
    icon_name: String,
    cookie: String,
    identities: Vec<Identity>,
    reply: rt::Sender<Result<(), String>>,
}

struct Active {
    request: Request,
    selected: usize,
    session: Option<Session>,
}

pub struct PolkitAgentRust {
    path: QString,
    is_registered: bool,
    owner: bool,
    token: u64,
    queue: VecDeque<Request>,
    active: Option<Active>,
    generation: u64,
}

impl Default for PolkitAgentRust {
    fn default() -> Self {
        Self {
            path: QString::default(),
            is_registered: false,
            owner: false,
            token: {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                NEXT.fetch_add(1, Ordering::Relaxed)
            },
            queue: VecDeque::new(),
            active: None,
            generation: 0,
        }
    }
}

impl Drop for PolkitAgentRust {
    fn drop(&mut self) {
        let reason = "PolkitAgent is being destroyed";
        for req in self.queue.drain(..) {
            req.reply.send(Err(reason.into()));
        }
        if let Some(active) = self.active.take() {
            active.request.reply.send(Err(reason.into()));
        }
        if self.owner {
            let mut target = TARGET.lock().unwrap();
            if target.as_ref().is_some_and(|(t, _)| *t == self.token) {
                *target = None;
            }
            TAKEN.store(false, Ordering::SeqCst);
        }
    }
}

impl qobject::PolkitAgentBase {
    fn set_path(mut self: Pin<&mut Self>, path: QString) {
        if self.path.is_empty() {
            self.as_mut().rust_mut().path = path;
        } else if self.path != path {
            quickshell::qs_log!(warn, LOG, "cannot change path after it has been set.");
        }
    }

    fn complete(mut self: Pin<&mut Self>) {
        if self.path.is_empty() {
            self.as_mut().rust_mut().path = QString::from("/org/quickshell/PolkitAgent");
        }
        if TAKEN.swap(true, Ordering::SeqCst) {
            return;
        }
        self.as_mut().rust_mut().owner = true;
        *TARGET.lock().unwrap() = Some((self.token, self.qt_thread()));
        if REGISTERED.load(Ordering::SeqCst) {
            self.as_mut().set_registered(true);
        }
        // A nested test shell shares the system bus and the user's session with the real one: it
        // must not take the session's polkit prompts where nobody can see them.
        if std::env::var_os("QSRS_NO_POLKIT_AGENT").is_some() {
            return;
        }
        let path = self.path.to_string();
        SERVING.call_once(|| {
            rt::spawn(async move {
                if let Err(e) = serve(path.clone()).await {
                    quickshell::qs_log!(warn, LOG, "failed to register listener on path {path}: {e}");
                }
            });
        });
    }

    fn set_registered(mut self: Pin<&mut Self>, registered: bool) {
        if self.is_registered != registered {
            self.as_mut().rust_mut().is_registered = registered;
            self.is_registered_changed();
        }
    }

    fn incoming(mut self: Pin<&mut Self>, request: Request) {
        quickshell::qs_log!(debug, LOG, "incoming authentication request for action {}", request.action_id);
        self.as_mut().rust_mut().queue.push_back(request);
        if self.active.is_none() {
            self.activate();
        }
    }

    fn activate(mut self: Pin<&mut Self>) {
        while let Some(request) = self.as_mut().rust_mut().queue.pop_front() {
            if request.identities.is_empty() {
                quickshell::qs_log!(warn, LOG, "no supported identities available for authentication request, cancelling.");
                request.reply.send(Err("Error requesting authentication: no supported identities available.".into()));
                continue;
            }
            let json = flow_json(&request);
            self.as_mut().rust_mut().active = Some(Active { request, selected: 0, session: None });
            self.as_mut().start_session();
            self.flow_started(QString::from(&json));
            return;
        }
    }

    /// A new conversation for the selected identity; events of older ones are dropped.
    fn start_session(mut self: Pin<&mut Self>) {
        let generation = self.generation + 1;
        let qt = self.qt_thread();
        let mut this = self.as_mut().rust_mut();
        this.generation = generation;
        let Some(active) = this.active.as_mut() else { return };
        active.session = None;
        let identity = &active.request.identities[active.selected];
        if identity.group {
            // The helper authenticates users only (libpolkit-agent: "Unsupported identity type").
            drop(this);
            return self.supplementary(QString::from("Cannot authenticate as a group"), true);
        }
        let emit = move |event| {
            let _ = qt.queue(move |this| this.session_event(generation, event));
        };
        match Session::start(&identity.name, &active.request.cookie, emit) {
            Ok(session) => active.session = Some(session),
            Err(e) => {
                quickshell::qs_log!(warn, LOG, "Cannot spawn helper: {e}");
                drop(this);
                self.supplementary(QString::from(&format!("Cannot start authentication: {e}")), true);
            }
        }
    }

    fn session_event(mut self: Pin<&mut Self>, generation: u64, event: Event) {
        if generation != self.generation || self.active.is_none() {
            return;
        }
        match event {
            Event::Request { prompt, echo } => self.prompt(QString::from(&prompt), echo),
            Event::Error(m) => self.supplementary(QString::from(&m), true),
            Event::Info(m) => self.supplementary(QString::from(&m), false),
            Event::Completed(true) => {
                let active = self.as_mut().rust_mut().active.take().unwrap();
                active.request.reply.send(Ok(()));
                self.as_mut().finished(true, false);
                self.end_flow();
            }
            Event::Completed(false) => {
                self.as_mut().failed();
                self.start_session();
            }
        }
    }

    fn end_flow(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().active = None;
        self.as_mut().rust_mut().generation += 1;
        self.as_mut().flow_ended();
        self.activate();
    }

    fn submit(mut self: Pin<&mut Self>, response: &QString) {
        if let Some(session) = self.as_mut().rust_mut().active.as_mut().and_then(|a| a.session.as_mut()) {
            session.respond(&response.to_string());
        }
    }

    fn cancel(mut self: Pin<&mut Self>) {
        let Some(active) = self.as_mut().rust_mut().active.take() else { return };
        quickshell::qs_log!(debug, LOG, "cancelling authentication request by user request");
        drop(active.session);
        active.request.reply.send(Err("Authentication request cancelled by user.".into()));
        self.as_mut().finished(false, false);
        self.end_flow();
    }

    fn cancel_from_agent(mut self: Pin<&mut Self>, cookie: String) {
        if self.active.as_ref().is_some_and(|a| a.request.cookie == cookie) {
            let active = self.as_mut().rust_mut().active.take().unwrap();
            drop(active.session);
            active.request.reply.send(Err("Authentication request cancelled by agent.".into()));
            self.as_mut().finished(false, true);
            self.end_flow();
        } else if let Some(i) = self.queue.iter().position(|r| r.cookie == cookie) {
            let request = self.as_mut().rust_mut().queue.remove(i).unwrap();
            request.reply.send(Err("Authentication request was cancelled".into()));
        } else {
            quickshell::qs_log!(warn, LOG, "the cancelled request was not found in the queue.");
        }
    }

    fn select_identity(mut self: Pin<&mut Self>, index: i32) {
        let Ok(index) = usize::try_from(index) else { return };
        let mut this = self.as_mut().rust_mut();
        let Some(active) = this.active.as_mut() else { return };
        if index >= active.request.identities.len() || index == active.selected {
            return;
        }
        active.selected = index;
        drop(this);
        self.start_session();
    }
}

fn flow_json(r: &Request) -> String {
    let mut out = String::from("{\"actionId\":");
    qs_dbus::json_string(&r.action_id, &mut out);
    out.push_str(",\"message\":");
    qs_dbus::json_string(&r.message, &mut out);
    out.push_str(",\"iconName\":");
    qs_dbus::json_string(&r.icon_name, &mut out);
    out.push_str(",\"cookie\":");
    qs_dbus::json_string(&r.cookie, &mut out);
    out.push_str(",\"identities\":[");
    for (i, id) in r.identities.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("{{\"id\":{},\"isGroup\":{},\"string\":", id.id, id.group));
        qs_dbus::json_string(&id.name, &mut out);
        out.push_str(",\"displayName\":");
        qs_dbus::json_string(&id.display_name, &mut out);
        out.push('}');
    }
    out.push_str("]}");
    out
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.PolicyKit1.Error")]
enum PolkitError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Failed(String),
    Cancelled(String),
}

type Subject = (String, HashMap<String, OwnedValue>);

struct Agent;

#[zbus::interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl Agent {
    async fn begin_authentication(
        &self,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        action_id: String,
        message: String,
        icon_name: String,
        _details: HashMap<String, String>,
        cookie: String,
        identities: Vec<Subject>,
    ) -> Result<(), PolkitError> {
        check_caller(conn, &hdr).await?;
        let mut seen = Vec::new();
        let mut list = Vec::new();
        for (kind, details) in identities {
            if seen.contains(&(kind.clone(), format!("{details:?}"))) {
                continue;
            }
            seen.push((kind.clone(), format!("{details:?}")));
            if let Some(identity) = identity(&kind, &details) {
                list.push(identity);
            }
        }
        let (reply, mut replied) = rt::channel();
        let request = Request { action_id, message, icon_name, cookie, identities: list, reply };
        if !target().is_some_and(|qt| qt.queue(move |this| this.incoming(request)).is_ok()) {
            return Err(PolkitError::Cancelled("PolkitAgent is being destroyed".into()));
        }
        match replied.recv().await {
            Some(Ok(())) => Ok(()),
            Some(Err(reason)) => Err(PolkitError::Cancelled(reason)),
            None => Err(PolkitError::Cancelled("PolkitAgent is being destroyed".into())),
        }
    }

    async fn cancel_authentication(
        &self,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        cookie: String,
    ) -> Result<(), PolkitError> {
        check_caller(conn, &hdr).await?;
        if let Some(qt) = target() {
            let _ = qt.queue(move |this| this.cancel_from_agent(cookie));
        }
        Ok(())
    }
}

/// Only polkitd talks to an agent: anyone else could put a password dialog on screen.
async fn check_caller(conn: &zbus::Connection, hdr: &zbus::message::Header<'_>) -> Result<(), PolkitError> {
    let dbus = zbus::fdo::DBusProxy::new(conn).await?;
    let owner = dbus.get_name_owner(POLKITD.try_into().unwrap()).await.map_err(zbus::Error::from)?;
    match hdr.sender() {
        Some(sender) if sender.as_str() == owner.as_str() => Ok(()),
        _ => Err(PolkitError::Failed("only polkitd may call the authentication agent".into())),
    }
}

/// unix-user and unix-group identities with names from the user and group databases, like
/// upstream Identity::fromPolkitIdentity; netgroups are skipped.
fn identity(kind: &str, details: &HashMap<String, OwnedValue>) -> Option<Identity> {
    let number = |key: &str| details.get(key).and_then(|v| u32::try_from(v).ok().or_else(|| i32::try_from(v).ok().map(|n| n as u32)));
    match kind {
        "unix-user" => {
            let uid = number("uid")?;
            let (name, gecos) = passwd(uid).unwrap_or_default();
            let name = if name.is_empty() { uid.to_string() } else { name };
            let display_name = if gecos.is_empty() { name.clone() } else { gecos };
            Some(Identity { id: uid, name, display_name, group: false })
        }
        "unix-group" => {
            let gid = number("gid")?;
            let name = group(gid).filter(|n| !n.is_empty()).unwrap_or_else(|| gid.to_string());
            Some(Identity { id: gid, display_name: name.clone(), name, group: true })
        }
        _ => None,
    }
}

fn passwd(uid: u32) -> Option<(String, String)> {
    let mut buf = vec![0 as libc::c_char; 16384];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) };
    if result.is_null() {
        return None;
    }
    let s = |p: *const libc::c_char| if p.is_null() { String::new() } else { unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned() };
    Some((s(pwd.pw_name), s(pwd.pw_gecos)))
}

fn group(gid: u32) -> Option<String> {
    let mut buf = vec![0 as libc::c_char; 16384];
    let mut grp: libc::group = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    unsafe { libc::getgrgid_r(gid, &mut grp, buf.as_mut_ptr(), buf.len(), &mut result) };
    if result.is_null() || grp.gr_name.is_null() {
        return None;
    }
    Some(unsafe { std::ffi::CStr::from_ptr(grp.gr_name) }.to_string_lossy().into_owned())
}

/// The logind session polkit_unix_session_new_for_process(getpid()) resolves to: the process's
/// own session, or for a process outside one (inir runs as a user service) the user's display
/// session, as polkitd does for its callers.
async fn session_id(conn: &zbus::Connection) -> zbus::Result<String> {
    let manager = zbus::Proxy::new(conn, LOGIND, "/org/freedesktop/login1", "org.freedesktop.login1.Manager").await?;
    let path: zbus::zvariant::OwnedObjectPath = match manager.call("GetSessionByPID", &(std::process::id(),)).await {
        Ok(p) => p,
        Err(_) => {
            let user: zbus::zvariant::OwnedObjectPath = manager.call("GetUser", &(unsafe { libc::getuid() },)).await?;
            let user = zbus::Proxy::new(conn, LOGIND, user, "org.freedesktop.login1.User").await?;
            let (id, _): (String, zbus::zvariant::OwnedObjectPath) = user.get_property("Display").await?;
            if id.is_empty() {
                return Err(zbus::Error::Failure("no session for this process and no display session".into()));
            }
            return Ok(id);
        }
    };
    let session = zbus::Proxy::new(conn, LOGIND, path, "org.freedesktop.login1.Session").await?;
    session.get_property("Id").await
}

async fn register(conn: &zbus::Connection, path: &str) -> zbus::Result<Subject> {
    let session = session_id(conn).await?;
    let subject: Subject = ("unix-session".into(), HashMap::from([("session-id".into(), OwnedValue::try_from(Value::from(session))?)]));
    let locale = std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".into());
    let authority = zbus::Proxy::new(conn, POLKITD, AUTHORITY, AUTHORITY_IFACE).await?;
    authority.call::<_, _, ()>("RegisterAuthenticationAgent", &(&subject, locale, path)).await?;
    Ok(subject)
}

enum Change {
    Owner(String),
    Retry,
}

/// Retry delay while another agent holds the session (a nested test shell on the same system
/// bus, or the previous shell still leaving): polkitd tells nobody when an agent goes away.
const RETRY: std::time::Duration = std::time::Duration::from_secs(5);

fn retry_later(tx: &rt::Sender<Change>) {
    let tx = tx.clone();
    std::thread::spawn(move || {
        std::thread::sleep(RETRY);
        tx.send(Change::Retry);
    });
}

/// Serves the agent for the life of the process (generations attach to it), registering again
/// whenever polkitd restarts, and until it gets the session when another agent held it first.
async fn serve(path: String) -> zbus::Result<()> {
    let conn = rt::system().await?;
    conn.object_server().at(path.as_str(), Agent).await?;
    let mut owners = qs_dbus::name_owner_changes(&conn, POLKITD).await?;
    let set = |ok: bool| {
        REGISTERED.store(ok, Ordering::SeqCst);
        if let Some(qt) = target() {
            let _ = qt.queue(move |this| this.set_registered(ok));
        }
    };
    let (tx, mut changes) = rt::channel();
    let owner_tx = tx.clone();
    rt::spawn(async move {
        while let Some(msg) = rt::next(&mut owners).await {
            let Ok(msg) = msg else { continue };
            if let Some((_, _, new_owner)) = qs_dbus::owner_change(&msg) {
                owner_tx.send(Change::Owner(new_owner));
            }
        }
    });
    let mut subject = None;
    let mut waiting = false;
    let mut pending = false;
    let mut change = Some(Change::Retry);
    while let Some(c) = change {
        match c {
            Change::Owner(new_owner) => {
                set(false);
                subject = None;
                waiting = false;
                if new_owner.is_empty() {
                    change = changes.recv().await;
                    continue;
                }
            }
            Change::Retry => {
                pending = false;
                if subject.is_some() {
                    change = changes.recv().await;
                    continue;
                }
            }
        }
        match register(&conn, &path).await {
            Ok(s) => {
                if waiting {
                    quickshell::qs_log!(info, LOG, "registered once the other agent left");
                }
                set(true);
                subject = Some(s);
                waiting = false;
            }
            Err(e) => {
                if !waiting {
                    quickshell::qs_log!(warn, LOG, "failed to register listener: {e}; retrying every {} s", RETRY.as_secs());
                }
                waiting = true;
                if !pending {
                    pending = true;
                    retry_later(&tx);
                }
            }
        }
        change = changes.recv().await;
    }
    if let Some(subject) = subject {
        let authority = zbus::Proxy::new(&conn, POLKITD, AUTHORITY, AUTHORITY_IFACE).await?;
        let _ = authority.call::<_, _, ()>("UnregisterAuthenticationAgent", &(&subject, path.as_str())).await;
    }
    conn.object_server().remove::<Agent, _>(path.as_str()).await?;
    Ok(())
}
