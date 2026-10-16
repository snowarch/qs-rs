//! D-Bus helpers shared by the service modules. Services run zbus' blocking API on their own
//! threads and hand values to QML as JSON text, which keeps nested D-Bus types (arrays of
//! dicts, structs) intact without wrapping each one in a QVariant.

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

/// A property value to write, typed the way the remote interface declares it.
pub enum SetValue {
    Bool(bool),
    U32(u32),
    Str(String),
}

/// org.freedesktop.DBus.Properties.Set, blocking.
pub fn set_property(conn: &zbus::blocking::Connection, service: &str, path: &str, iface: &str, name: &str, value: SetValue) -> zbus::Result<()> {
    let proxy = zbus::blocking::Proxy::new(conn, service.to_string(), path.to_string(), "org.freedesktop.DBus.Properties")?;
    let value = match value {
        SetValue::Bool(b) => Value::Bool(b),
        SetValue::U32(u) => Value::U32(u),
        SetValue::Str(s) => Value::from(s),
    };
    proxy.call("Set", &(iface, name, value))
}

/// Calls a method with no arguments, or with one object path argument.
pub fn call(conn: &zbus::blocking::Connection, service: &str, path: &str, iface: &str, method: &str, object_arg: Option<&str>) -> zbus::Result<()> {
    let proxy = zbus::blocking::Proxy::new(conn, service.to_string(), path.to_string(), iface.to_string())?;
    match object_arg {
        Some(p) => proxy.call(method, &(zbus::zvariant::ObjectPath::try_from(p)?,)),
        None => proxy.call(method, &()),
    }
}
