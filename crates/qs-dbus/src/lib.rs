//! D-Bus helpers shared by the service modules. Every service runs as a task on the one D-Bus
//! thread (`rt`) over the shared connection of its bus, and hands values to QML as JSON text,
//! which keeps nested D-Bus types (arrays of dicts, structs) intact without wrapping each one in
//! a QVariant.

pub use zbus;
use zbus::zvariant::Value;

pub fn json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_f64(v: f64, out: &mut String) {
    if v.is_finite() { out.push_str(&v.to_string()) } else { out.push_str("null") }
}

/// Writes `value` as JSON. Dict keys become strings, structs become arrays.
pub fn write_json(value: &Value, out: &mut String) {
    match value {
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::U8(v) => out.push_str(&v.to_string()),
        Value::I16(v) => out.push_str(&v.to_string()),
        Value::U16(v) => out.push_str(&v.to_string()),
        Value::I32(v) => out.push_str(&v.to_string()),
        Value::U32(v) => out.push_str(&v.to_string()),
        Value::I64(v) => out.push_str(&v.to_string()),
        Value::U64(v) => out.push_str(&v.to_string()),
        Value::F64(v) => json_f64(*v, out),
        Value::Str(s) => json_string(s, out),
        Value::Signature(s) => json_string(&s.to_string(), out),
        Value::ObjectPath(p) => json_string(p.as_str(), out),
        Value::Value(v) => write_json(v, out),
        Value::Array(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 { out.push(',') }
                write_json(v, out);
            }
            out.push(']');
        }
        Value::Dict(d) => {
            out.push('{');
            for (i, (k, v)) in d.iter().enumerate() {
                if i > 0 { out.push(',') }
                match k {
                    Value::Str(s) => json_string(s, out),
                    other => {
                        let mut key = String::new();
                        write_json(other, &mut key);
                        json_string(key.trim_matches('"'), out);
                    }
                }
                out.push(':');
                write_json(v, out);
            }
            out.push('}');
        }
        Value::Structure(s) => {
            out.push('[');
            for (i, v) in s.fields().iter().enumerate() {
                if i > 0 { out.push(',') }
                write_json(v, out);
            }
            out.push(']');
        }
        #[allow(unreachable_patterns)]
        _ => out.push_str("null"),
    }
}

/// A property map (`a{sv}`) as a JSON object.
pub fn props_json<'a, K: AsRef<str>>(props: impl IntoIterator<Item = (K, &'a Value<'a>)>) -> String {
    let mut out = String::from("{");
    for (i, (k, v)) in props.into_iter().enumerate() {
        if i > 0 { out.push(',') }
        json_string(k.as_ref(), &mut out);
        out.push(':');
        write_json(v, &mut out);
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use zbus::zvariant::OwnedValue;

    #[test]
    fn nested_values() {
        let mut inner: HashMap<String, Value> = HashMap::new();
        inner.insert("Profile".into(), Value::from("balanced"));
        let list = Value::from(vec![inner]);
        let mut out = String::new();
        write_json(&list, &mut out);
        assert_eq!(out, r#"[{"Profile":"balanced"}]"#);
        let props: Vec<(String, OwnedValue)> =
            vec![("Percentage".into(), Value::F64(42.5).try_into().unwrap()), ("Model".into(), Value::from("a\"b").try_into().unwrap())];
        assert_eq!(props_json(props.iter().map(|(k, v)| (k, &**v))), r#"{"Percentage":42.5,"Model":"a\"b"}"#);
    }
}

pub mod object_manager;
pub mod pixmap;
pub mod rt;

/// A property value to write, typed the way the remote interface declares it.
pub enum SetValue {
    Bool(bool),
    U32(u32),
    F64(f64),
    Str(String),
}

/// org.freedesktop.DBus.Properties.Set.
pub async fn set_property(conn: &zbus::Connection, service: &str, path: &str, iface: &str, name: &str, value: SetValue) -> zbus::Result<()> {
    let proxy = zbus::Proxy::new(conn, service.to_string(), path.to_string(), "org.freedesktop.DBus.Properties").await?;
    let value = match value {
        SetValue::Bool(b) => Value::Bool(b),
        SetValue::U32(u) => Value::U32(u),
        SetValue::F64(f) => Value::F64(f),
        SetValue::Str(s) => Value::from(s),
    };
    proxy.call("Set", &(iface, name, value)).await
}

/// Calls a method with no arguments, or with one object path argument.
pub async fn call(conn: &zbus::Connection, service: &str, path: &str, iface: &str, method: &str, object_arg: Option<&str>) -> zbus::Result<()> {
    let proxy = zbus::Proxy::new(conn, service.to_string(), path.to_string(), iface.to_string()).await?;
    match object_arg {
        Some(p) => proxy.call(method, &(zbus::zvariant::ObjectPath::try_from(p)?,)).await,
        None => proxy.call(method, &()).await,
    }
}

/// org.freedesktop.DBus.Properties.GetAll.
pub async fn get_all(conn: &zbus::Connection, service: &str, path: &str, iface: &str) -> zbus::Result<std::collections::HashMap<String, zbus::zvariant::OwnedValue>> {
    let proxy = zbus::Proxy::new(conn, service.to_string(), path.to_string(), "org.freedesktop.DBus.Properties").await?;
    proxy.call("GetAll", &(iface,)).await
}

/// Which shared connection a service talks over.
#[derive(Clone, Copy)]
pub enum Bus {
    Session,
    System,
}

impl Bus {
    pub async fn connect(self) -> zbus::Result<zbus::Connection> {
        match self {
            Bus::Session => rt::session().await,
            Bus::System => rt::system().await,
        }
    }
}

type Job = Box<dyn FnOnce(zbus::Connection) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send>;

/// Runs bus calls in the order they were requested (one task on the D-Bus thread), so a quick
/// play then pause can't reach the remote side swapped. Calls queued before the bus connects
/// run once it does.
pub struct Worker(rt::Sender<Job>);

impl Worker {
    pub fn new(bus: Bus, who: &'static str) -> Self {
        let (tx, mut rx) = rt::channel::<Job>();
        rt::spawn(async move {
            let conn = match bus.connect().await {
                Ok(c) => c,
                Err(e) => return quickshell::qs_log!(warn, "quickshell.dbus", "{who}: cannot connect to the bus: {e}"),
            };
            while let Some(job) = rx.recv().await {
                job(conn.clone()).await;
            }
        });
        Self(tx)
    }

    /// Queues `f`; an error is logged as "`what` failed".
    pub fn run<F>(&self, what: String, f: impl FnOnce(zbus::Connection) -> F + Send + 'static)
    where
        F: std::future::Future<Output = zbus::Result<()>> + Send + 'static,
    {
        self.0.send(Box::new(move |conn| {
            Box::pin(async move {
                if let Err(e) = f(conn).await {
                    quickshell::qs_log!(warn, "quickshell.dbus", "{what} failed: {e}");
                }
            })
        }));
    }
}

/// Owns `name` without queueing (like QDBusConnection::registerService) and claims it again each
/// time its owner goes away. Runs for the life of the connection.
pub async fn claim_name(conn: &zbus::Connection, name: &str, who: &str) -> zbus::Result<()> {
    use zbus::fdo::RequestNameFlags;
    let mut changes = name_owner_changes(conn, name).await?;
    let try_claim = || async {
        match conn.request_name_with_flags(name, RequestNameFlags::DoNotQueue.into()).await {
            Ok(_) => quickshell::qs_log!(info, "quickshell.dbus", "{who}: registered {name}"),
            Err(zbus::Error::NameTaken) => {
                quickshell::qs_log!(warn, "quickshell.dbus", "{who}: {name} is owned by another service; registration will be retried when it goes away")
            }
            Err(e) => quickshell::qs_log!(warn, "quickshell.dbus", "{who}: cannot register {name}: {e}"),
        }
    };
    try_claim().await;
    while let Some(msg) = rt::next(&mut changes).await {
        if owner_change(&msg?).is_some_and(|(_, _, new)| new.is_empty()) {
            try_claim().await;
        }
    }
    Ok(())
}

/// NameOwnerChanged signals for one name or (empty `name`) for all of them; read each with
/// `owner_change`.
pub async fn name_owner_changes(conn: &zbus::Connection, name: &str) -> zbus::Result<zbus::MessageStream> {
    let mut rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?;
    if !name.is_empty() {
        rule = rule.arg(0, name.to_string())?;
    }
    signals(conn, rule.build()).await
}

/// `(name, old_owner, new_owner)` of a NameOwnerChanged signal.
pub fn owner_change(msg: &zbus::Message) -> Option<(String, String, String)> {
    msg.body().deserialize::<(String, String, String)>().ok()
}

/// Every message matching `rule`.
pub async fn signals(conn: &zbus::Connection, rule: zbus::MatchRule<'static>) -> zbus::Result<zbus::MessageStream> {
    zbus::MessageStream::for_match_rule(rule, conn, None).await
}

/// Every signal from `sender`.
pub async fn signals_from(conn: &zbus::Connection, sender: &str) -> zbus::Result<zbus::MessageStream> {
    signals(conn, zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).sender(sender.to_string())?.build()).await
}
