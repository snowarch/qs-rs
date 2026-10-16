//! Client for a service exposing org.freedesktop.DBus.ObjectManager (BlueZ, UDisks, ...):
//! GetManagedObjects, then InterfacesAdded/Removed and PropertiesChanged from the service.

use crate::props_json;
use std::collections::HashMap;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::Type;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::MatchRule;

pub enum Event {
    /// Object path and `{interface: {property: value}}` as JSON.
    InterfacesAdded(String, String),
    /// Object path and the removed interface names.
    InterfacesRemoved(String, Vec<String>),
    /// Object path, interface, changed properties as JSON.
    PropertiesChanged(String, String, String),
}

type Props = HashMap<String, OwnedValue>;

fn interfaces_json(ifaces: &HashMap<String, Props>) -> String {
    let mut out = String::from("{");
    for (i, (name, props)) in ifaces.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        crate::json_string(name, &mut out);
        out.push(':');
        out.push_str(&props_json(props.iter().map(|(k, v)| (k, &**v))));
    }
    out.push('}');
    out
}

/// Blocks forever on the calling thread, reporting through `emit`.
pub fn run(conn: &Connection, service: &str, root: &str, emit: impl Fn(Event)) -> zbus::Result<()> {
    let rule = MatchRule::builder().msg_type(Type::Signal).sender(service.to_string())?.build();
    let stream = MessageIterator::for_match_rule(rule, conn, None)?;
    let manager = Proxy::new(conn, service.to_string(), root.to_string(), "org.freedesktop.DBus.ObjectManager")?;
    let objects: HashMap<OwnedObjectPath, HashMap<String, Props>> = manager.call("GetManagedObjects", &())?;
    let mut sorted: Vec<_> = objects.iter().collect();
    sorted.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    for (path, ifaces) in sorted {
        emit(Event::InterfacesAdded(path.to_string(), interfaces_json(ifaces)));
    }
    for msg in stream {
        let msg = msg?;
        let header = msg.header();
        let (Some(member), Some(path)) = (header.member(), header.path()) else { continue };
        let body = msg.body();
        match member.as_str() {
            "InterfacesAdded" => {
                if let Ok((p, ifaces)) = body.deserialize::<(OwnedObjectPath, HashMap<String, Props>)>() {
                    emit(Event::InterfacesAdded(p.to_string(), interfaces_json(&ifaces)));
                }
            }
            "InterfacesRemoved" => {
                if let Ok((p, ifaces)) = body.deserialize::<(OwnedObjectPath, Vec<String>)>() {
                    emit(Event::InterfacesRemoved(p.to_string(), ifaces));
                }
            }
            "PropertiesChanged" => {
                if let Ok((iface, changed, _)) = body.deserialize::<(String, Props, Vec<String>)>() {
                    emit(Event::PropertiesChanged(path.to_string(), iface, props_json(changed.iter().map(|(k, v)| (k, &**v)))));
                }
            }
            _ => {}
        }
    }
    Ok(())
}
