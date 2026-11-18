//! Moving apps onto fabrial's mic, so they can keep picking the headset.
//!
//! When WirePlumber links an app's recording stream to the mic fabrial
//! reads, fabrial sets that stream's `target.object` in the "default"
//! metadata to its own mic, and WirePlumber relinks it. That's how
//! `pactl move-source-output` and EasyEffects move streams. WirePlumber also
//! remembers the choice per app: while fabrial runs, the app's next streams
//! go straight to it, and when it doesn't, to the default mic.
//!
//! Left alone:
//! - streams that refuse to move (`node.dont-move`), like recorders;
//! - level meters (`stream.monitor`);
//! - streams on any other mic, EasyEffects' included;
//! - fabrial's own.
//!
//! A stream is moved once: if it's moved back, it stays there. On exit, the
//! targets fabrial set are removed, so those streams return to the mic they
//! had.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use pipewire as pw;
use pw::metadata::Metadata;
use pw::node::{Node, NodeChangeMask, NodeListener};
use pw::registry::{GlobalObject, Listener, RegistryRc};
use pw::spa::utils::dict::DictRef;
use pw::types::ObjectType;

/// fabrial's two node ids, (input, virtual mic). PipeWire assigns them a
/// little after the streams connect, so the streams fill this in.
pub type OwnIds = Rc<Cell<(u32, u32)>>;

pub struct Mover {
    state: Rc<RefCell<State>>,
    _listener: Listener,
    _registry: RegistryRc,
}

struct State {
    own: OwnIds,
    /// Every node's (name, object.serial).
    nodes: HashMap<u32, (String, String)>,
    /// Links, as (output node, input node).
    links: HashMap<u32, (u32, u32)>,
    metadata: Option<Metadata>,
    /// Other apps' recording streams.
    streams: HashMap<u32, AppStream>,
    moved: HashSet<u32>,
    /// The mic last reported as read, to report changes.
    reported_mic: Option<u32>,
}

struct AppStream {
    app: String,
    /// Known once the stream's full properties arrive.
    movable: Option<bool>,
    _node: Node,
    _listener: Option<NodeListener>,
}

impl Mover {
    pub fn new(core: &pw::core::CoreRc, own: OwnIds) -> Result<Self, pw::Error> {
        let registry = core.get_registry_rc()?;
        let state = Rc::new(RefCell::new(State {
            own,
            nodes: HashMap::new(),
            links: HashMap::new(),
            metadata: None,
            streams: HashMap::new(),
            moved: HashSet::new(),
            reported_mic: None,
        }));
        let listener = registry
            .add_listener_local()
            .global({
                let registry = registry.downgrade();
                let state = Rc::downgrade(&state);
                move |global| {
                    if let (Some(registry), Some(state)) = (registry.upgrade(), state.upgrade()) {
                        added(&registry, &state, global);
                    }
                }
            })
            .global_remove({
                let state = Rc::downgrade(&state);
                move |id| {
                    if let Some(state) = state.upgrade() {
                        state.borrow_mut().removed(id);
                    }
                }
            })
            .register();
        Ok(Self {
            state,
            _listener: listener,
            _registry: registry,
        })
    }

    /// Looks again, now that something changed (fabrial's ids are known).
    pub fn poke(&self) {
        self.state.borrow_mut().consider_all();
    }

    /// Removes the targets fabrial set. They reach PipeWire on the next
    /// round trip.
    pub fn restore(&self) {
        let mut st = self.state.borrow_mut();
        if let Some(md) = &st.metadata {
            for &id in &st.moved {
                md.set_property(id, "target.object", None, None);
            }
        }
        st.moved.clear();
    }
}

fn added(registry: &RegistryRc, state: &Rc<RefCell<State>>, global: &GlobalObject<&DictRef>) {
    let get = |key: &str| global.props.and_then(|p| p.get(key));
    let id = global.id;
    let mut st = state.borrow_mut();
    match global.type_ {
        ObjectType::Node => {
            let name = get("node.name").unwrap_or("?").to_string();
            let serial = get("object.serial").unwrap_or("").to_string();
            st.nodes.insert(id, (name, serial));
            if get("media.class") == Some("Stream/Input/Audio") {
                watch_stream(registry, state, &mut st, global);
            }
            st.consider_all();
        }
        ObjectType::Link => {
            let node = |key| get(key).and_then(|v| v.parse::<u32>().ok());
            if let (Some(out), Some(inp)) = (node("link.output.node"), node("link.input.node")) {
                st.links.insert(id, (out, inp));
                st.consider_all();
            }
        }
        ObjectType::Metadata if get("metadata.name") == Some("default") => {
            if let Ok(md) = registry.bind::<Metadata, _>(global) {
                st.metadata = Some(md);
                st.consider_all();
            }
        }
        _ => {}
    }
}

/// Keeps a recording stream, and asks for its full properties: the
/// registry doesn't show `node.dont-move` or `stream.monitor`.
fn watch_stream(
    registry: &RegistryRc,
    state: &Rc<RefCell<State>>,
    st: &mut State,
    global: &GlobalObject<&DictRef>,
) {
    let id = global.id;
    let Ok(node) = registry.bind::<Node, _>(global) else {
        return;
    };
    let listener = node
        .add_listener_local()
        .info({
            let state = Rc::downgrade(state);
            move |info| {
                // Later events carry no properties unless they changed.
                if !info.change_mask().contains(NodeChangeMask::PROPS) {
                    return;
                }
                let Some(props) = info.props() else {
                    return;
                };
                let set = |key| props.get(key).is_some_and(|v| v == "true" || v == "1");
                let movable = !set("node.dont-move") && !set("stream.monitor");
                if let Some(state) = Weak::upgrade(&state) {
                    let mut st = state.borrow_mut();
                    if let Some(s) = st.streams.get_mut(&id) {
                        s.movable = Some(movable);
                    }
                    st.consider_all();
                }
            }
        })
        .register();
    let props = global.props;
    let app = props
        .and_then(|p| p.get("application.name").or(p.get("node.name")))
        .unwrap_or("?")
        .to_string();
    st.streams.insert(
        id,
        AppStream {
            app,
            movable: None,
            _node: node,
            _listener: Some(listener),
        },
    );
}

impl State {
    fn removed(&mut self, id: u32) {
        self.nodes.remove(&id);
        self.links.remove(&id);
        self.streams.remove(&id);
        self.moved.remove(&id);
    }

    /// The mic fabrial reads: whatever its input is linked from.
    fn mic(&self) -> Option<u32> {
        let input = self.own.get().0;
        self.links
            .values()
            .find(|&&(_, i)| i == input)
            .map(|&(o, _)| o)
    }

    fn consider_all(&mut self) {
        let mic = self.mic();
        if mic != self.reported_mic {
            if let Some(name) = mic.and_then(|m| self.nodes.get(&m)).map(|n| &n.0) {
                eprintln!("fabrial: reading {name}");
            }
            self.reported_mic = mic;
        }
        let (input, source) = self.own.get();
        let (Some(mic), Some(md)) = (mic, &self.metadata) else {
            return;
        };
        let Some(serial) = self.nodes.get(&source).map(|n| n.1.as_str()) else {
            return;
        };
        for (&id, stream) in &self.streams {
            if id == input
                || self.moved.contains(&id)
                || stream.movable != Some(true)
                || !self.links.values().any(|&l| l == (mic, id))
            {
                continue;
            }
            md.set_property(id, "target.object", Some("Spa:Id"), Some(serial));
            eprintln!("fabrial: moved {} onto the fabrial mic", stream.app);
            self.moved.insert(id);
        }
    }
}
