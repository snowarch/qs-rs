//! PipeWire connection on its own thread. Tracks nodes (with their Props: volumes, channel map,
//! mute), links, devices (Route params, for hardware volume) and the `default` metadata, and
//! reports changes through a callback. Commands arrive over a pipewire channel.

use pipewire as pw;
use pw::spa;
use pw::types::ObjectType;
use spa::param::ParamType;
use spa::pod::deserialize::PodDeserializer;
use spa::pod::serialize::PodSerializer;
use spa::pod::{Object, Pod, Property, Value, ValueArray};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Update {
    Node { id: u32, props: Vec<(String, String)> },
    NodeAudio { id: u32, volumes: Vec<f32>, channels: Vec<u32>, muted: bool },
    NodeRemoved(u32),
    Link { id: u32, output_node: u32, input_node: u32, state: i32 },
    LinkRemoved(u32),
    /// `default.audio.sink`, `default.configured.audio.source`, ... -> node name
    Default { key: String, name: String },
    Ready,
}

#[derive(Debug)]
pub enum Command {
    SetVolumes { node: u32, volumes: Vec<f32> },
    SetMuted { node: u32, muted: bool },
    SetDefault { key: String, name: String },
}

const PROP_MUTE: u32 = spa::sys::SPA_PROP_mute;
const PROP_CHANNEL_VOLUMES: u32 = spa::sys::SPA_PROP_channelVolumes;
const PROP_CHANNEL_MAP: u32 = spa::sys::SPA_PROP_channelMap;

fn serialize(value: &Value) -> Vec<u8> {
    PodSerializer::serialize(std::io::Cursor::new(Vec::new()), value).map(|(c, _)| c.into_inner()).unwrap_or_default()
}

fn props_object(props: Vec<Property>) -> Value {
    Value::Object(Object { type_: spa::sys::SPA_TYPE_OBJECT_Props, id: spa::sys::SPA_PARAM_Props, properties: props })
}

fn volumes_prop(volumes: &[f32]) -> Property {
    Property::new(PROP_CHANNEL_VOLUMES, Value::ValueArray(ValueArray::Float(volumes.iter().map(|v| v * v * v).collect())))
}

/// (cbrt volumes, channel ids, mute) from a Props object.
fn parse_props(pod: &Pod) -> Option<(Vec<f32>, Vec<u32>, Option<bool>)> {
    let (_, value) = PodDeserializer::deserialize_any_from(pod.as_bytes()).ok()?;
    let Value::Object(obj) = value else { return None };
    let (mut volumes, mut channels, mut muted) = (None, Vec::new(), None);
    for p in obj.properties {
        match (p.key, p.value) {
            (PROP_CHANNEL_VOLUMES, Value::ValueArray(ValueArray::Float(v))) => volumes = Some(v.iter().map(|x| x.cbrt()).collect()),
            (PROP_CHANNEL_MAP, Value::ValueArray(ValueArray::Id(v))) => channels = v.iter().map(|i| i.0).collect(),
            (PROP_MUTE, Value::Bool(b)) => muted = Some(b),
            _ => {}
        }
    }
    Some((volumes?, channels, muted))
}

/// route device -> route index, from a device Route param.
fn parse_route(pod: &Pod) -> Option<(i32, i32)> {
    let (_, value) = PodDeserializer::deserialize_any_from(pod.as_bytes()).ok()?;
    let Value::Object(obj) = value else { return None };
    let (mut device, mut index) = (None, None);
    for p in obj.properties {
        match (p.key, p.value) {
            (spa::sys::SPA_PARAM_ROUTE_device, Value::Int(d)) => device = Some(d),
            (spa::sys::SPA_PARAM_ROUTE_index, Value::Int(i)) => index = Some(i),
            _ => {}
        }
    }
    Some((device?, index?))
}

/// `name` of a default-node metadata value. Accepts strict JSON (`{"name":"x"}`, what
/// WirePlumber writes) and SPA's relaxed JSON (`{ name = x }`).
fn json_name(value: &str) -> Option<String> {
    let start = value.find("name")? + 4;
    let rest = value[start..].strip_prefix('"').unwrap_or(&value[start..]);
    let rest = rest.trim_start().strip_prefix([':', '='])?.trim_start();
    let name = match rest.strip_prefix('"') {
        Some(quoted) => &quoted[..quoted.find('"')?],
        None => &rest[..rest.find(|c: char| c == ',' || c == '}' || c.is_whitespace()).unwrap_or(rest.len())],
    };
    (!name.is_empty()).then(|| name.to_string())
}

struct NodeEntry {
    proxy: pw::node::Node,
    _listener: pw::node::NodeListener,
    device: Option<u32>,
    route_device: Option<i32>,
    muted: bool,
}

struct DeviceEntry {
    proxy: pw::device::Device,
    _listener: pw::device::DeviceListener,
    routes: HashMap<i32, i32>,
}

#[derive(Default)]
struct State {
    nodes: HashMap<u32, NodeEntry>,
    devices: HashMap<u32, DeviceEntry>,
    links: HashMap<u32, (pw::link::Link, pw::link::LinkListener)>,
    metadata: Option<(pw::metadata::Metadata, pw::metadata::MetadataListener)>,
}

/// Runs the PipeWire loop forever on the calling thread.
pub fn run(emit: impl Fn(Update) + 'static, commands: pw::channel::Receiver<Command>) -> Result<(), pw::Error> {
    pw::init();
    let main_loop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&main_loop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let emit = Rc::new(emit);
    let state = Rc::new(RefCell::new(State::default()));

    let _cmd = commands.attach(main_loop.loop_(), {
        let state = state.clone();
        move |cmd| handle_command(&state.borrow(), cmd)
    });

    let reg_weak = registry.downgrade();
    let (st, em) = (state.clone(), emit.clone());
    let _reg_listener = registry
        .add_listener_local()
        .global(move |obj| {
            let Some(registry) = reg_weak.upgrade() else { return };
            let props: Vec<(String, String)> =
                obj.props.map(|d| d.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()).unwrap_or_default();
            let prop = |k: &str| props.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.clone());
            match obj.type_ {
                ObjectType::Node => {
                    let Ok(node) = registry.bind::<pw::node::Node, _>(obj) else { return };
                    let id = obj.id;
                    let (st2, em2) = (st.clone(), em.clone());
                    let em3 = em.clone();
                    let listener = node
                        .add_listener_local()
                        .info(move |info| {
                            // Later info events carry an empty dict unless the props changed.
                            if !info.change_mask().contains(pw::node::NodeChangeMask::PROPS) {
                                return;
                            }
                            if let Some(d) = info.props() {
                                let props: Vec<(String, String)> = d.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
                                let get = |k: &str| props.iter().find(|(pk, _)| pk == k).and_then(|(_, v)| v.parse::<i64>().ok());
                                if let Some(n) = st2.borrow_mut().nodes.get_mut(&id) {
                                    n.device = get("device.id").map(|v| v as u32);
                                    n.route_device = get("card.profile.device").map(|v| v as i32);
                                }
                                em2(Update::Node { id, props });
                            }
                        })
                        .param(move |_seq, _id, _index, _next, param| {
                            if let Some((volumes, channels, muted)) = param.and_then(parse_props) {
                                em3(Update::NodeAudio { id, volumes, channels, muted: muted.unwrap_or(false) });
                            }
                        })
                        .register();
                    node.subscribe_params(&[ParamType::Props]);
                    em(Update::Node { id, props: props.clone() });
                    st.borrow_mut().nodes.insert(
                        id,
                        NodeEntry {
                            proxy: node,
                            _listener: listener,
                            device: prop("device.id").and_then(|v| v.parse().ok()),
                            route_device: prop("card.profile.device").and_then(|v| v.parse().ok()),
                            muted: false,
                        },
                    );
                }
                ObjectType::Device => {
                    let Ok(device) = registry.bind::<pw::device::Device, _>(obj) else { return };
                    let id = obj.id;
                    let st2 = st.clone();
                    let listener = device
                        .add_listener_local()
                        .param(move |_seq, kind, _index, _next, param| {
                            if kind == ParamType::Route
                                && let Some((dev, index)) = param.and_then(parse_route)
                                    && let Some(d) = st2.borrow_mut().devices.get_mut(&id) {
                                        d.routes.insert(dev, index);
                                    }
                        })
                        .register();
                    device.subscribe_params(&[ParamType::Route]);
                    st.borrow_mut().devices.insert(id, DeviceEntry { proxy: device, _listener: listener, routes: HashMap::new() });
                }
                ObjectType::Link => {
                    let Ok(link) = registry.bind::<pw::link::Link, _>(obj) else { return };
                    let id = obj.id;
                    let em2 = em.clone();
                    let listener = link
                        .add_listener_local()
                        .info(move |info| {
                            em2(Update::Link {
                                id,
                                output_node: info.output_node_id(),
                                input_node: info.input_node_id(),
                                state: link_state(info.state()),
                            });
                        })
                        .register();
                    st.borrow_mut().links.insert(id, (link, listener));
                }
                ObjectType::Metadata if prop("metadata.name").as_deref() == Some("default") => {
                    let Ok(meta) = registry.bind::<pw::metadata::Metadata, _>(obj) else { return };
                    let em2 = em.clone();
                    let listener = meta
                        .add_listener_local()
                        .property(move |_subject, key, _type, value| {
                            if let Some(key) = key.filter(|k| k.starts_with("default.")) {
                                em2(Update::Default { key: key.to_string(), name: value.and_then(json_name).unwrap_or_default() });
                            }
                            0
                        })
                        .register();
                    st.borrow_mut().metadata = Some((meta, listener));
                }
                _ => {}
            }
        })
        .global_remove({
            let (st, em) = (state.clone(), emit.clone());
            move |id| {
                let mut s = st.borrow_mut();
                if s.nodes.remove(&id).is_some() {
                    em(Update::NodeRemoved(id));
                } else if s.links.remove(&id).is_some() {
                    em(Update::LinkRemoved(id));
                } else {
                    s.devices.remove(&id);
                }
            }
        })
        .register();

    // Everything announced by the first roundtrip is the initial state.
    let pending = core.sync(0)?;
    let em = emit.clone();
    let _core_listener = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == pw::core::PW_ID_CORE && seq == pending {
                em(Update::Ready);
            }
        })
        .register();

    main_loop.run();
    Ok(())
}

fn link_state(state: pw::link::LinkState) -> i32 {
    use pw::link::LinkState as S;
    match state {
        S::Error(_) => -2,
        S::Unlinked => -1,
        S::Init => 0,
        S::Negotiating => 1,
        S::Allocating => 2,
        S::Paused => 3,
        S::Active => 4,
    }
}

fn handle_command(state: &State, cmd: Command) {
    match cmd {
        Command::SetVolumes { node, volumes } => {
            let Some(n) = state.nodes.get(&node) else { return };
            let via_device = n.device.zip(n.route_device).and_then(|(d, rd)| {
                let dev = state.devices.get(&d)?;
                Some((dev, rd, *dev.routes.get(&rd)?))
            });
            match via_device {
                Some((dev, route_device, index)) => set_route_props(dev, route_device, index, vec![volumes_prop(&volumes)]),
                None => n.proxy.set_param(ParamType::Props, 0, Pod::from_bytes(&serialize(&props_object(vec![volumes_prop(&volumes)]))).unwrap()),
            }
        }
        Command::SetMuted { node, muted } => {
            let Some(n) = state.nodes.get(&node) else { return };
            let _ = n.muted;
            let prop = Property::new(PROP_MUTE, Value::Bool(muted));
            let via_device = n.device.zip(n.route_device).and_then(|(d, rd)| {
                let dev = state.devices.get(&d)?;
                Some((dev, rd, *dev.routes.get(&rd)?))
            });
            match via_device {
                Some((dev, route_device, index)) => set_route_props(dev, route_device, index, vec![prop]),
                None => n.proxy.set_param(ParamType::Props, 0, Pod::from_bytes(&serialize(&props_object(vec![prop]))).unwrap()),
            }
        }
        Command::SetDefault { key, name } => {
            if let Some((meta, _)) = &state.metadata {
                meta.set_property(0, &key, Some("Spa:String:JSON"), Some(&format!("{{\"name\":\"{name}\"}}")));
            }
        }
    }
}

fn set_route_props(dev: &DeviceEntry, route_device: i32, index: i32, props: Vec<Property>) {
    let route = Value::Object(Object {
        type_: spa::sys::SPA_TYPE_OBJECT_ParamRoute,
        id: spa::sys::SPA_PARAM_Route,
        properties: vec![
            Property::new(spa::sys::SPA_PARAM_ROUTE_device, Value::Int(route_device)),
            Property::new(spa::sys::SPA_PARAM_ROUTE_index, Value::Int(index)),
            Property::new(spa::sys::SPA_PARAM_ROUTE_props, props_object(props)),
            Property::new(spa::sys::SPA_PARAM_ROUTE_save, Value::Bool(true)),
        ],
    });
    if let Some(pod) = Pod::from_bytes(&serialize(&route)) {
        dev.proxy.set_param(ParamType::Route, 0, pod);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn json_name_extracts() {
        assert_eq!(super::json_name(r#"{ "name": "alsa_output.pci" }"#).as_deref(), Some("alsa_output.pci"));
        assert_eq!(super::json_name(r#"{"name":"a b"}"#).as_deref(), Some("a b"));
        assert_eq!(super::json_name("{ name = qsrs-test-sink }").as_deref(), Some("qsrs-test-sink"));
        assert_eq!(super::json_name("{ name: x, other = 1 }").as_deref(), Some("x"));
        assert_eq!(super::json_name("{}"), None);
    }
}
