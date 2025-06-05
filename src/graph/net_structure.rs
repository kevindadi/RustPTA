use crate::concurrency::atomic::AtomicOrdering;
use crate::memory::pointsto::AliasId;

use super::callgraph::InstanceId;
use petgraph::graph::NodeIndex;
use petgraph::Graph;
use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;
use thiserror::Error;

#[derive(Debug, Clone)]
pub enum Shape {
    Circle,
    Box,
}

#[derive(Debug, Clone)]
pub struct Place {
    pub name: String,
    pub tokens: Rc<RefCell<u8>>,
    pub capacity: u8,
    pub span: String,
    pub place_type: PlaceType,
}

type PetriNetGraph = Graph<PetriNetNode, PetriNetEdge>;

#[derive(Debug, Clone, Eq, PartialEq, PartialOrd, Ord)]
pub enum PlaceType {
    Resources,
    FunctionStart,
    FunctionEnd,
    BasicBlock,
}

impl Place {
    pub fn new(name: String, token: u8, place_type: PlaceType) -> Self {
        Self {
            name,
            tokens: Rc::new(RefCell::new(token)),
            capacity: token,
            span: String::new(),
            place_type,
        }
    }

    pub fn new_with_span(name: String, token: u8, place_type: PlaceType, span: String) -> Self {
        Self {
            name,
            tokens: Rc::new(RefCell::new(token)),
            capacity: 1u8,
            span,
            place_type,
        }
    }

    pub fn new_with_no_token(name: String, place_type: PlaceType) -> Self {
        Self {
            name,
            tokens: Rc::new(RefCell::new(0)),
            capacity: 1u8,
            span: String::new(),
            place_type,
        }
    }

    pub fn new_indefinite(
        name: String,
        token: u8,
        capacity: u8,
        place_type: PlaceType,
        span: String,
    ) -> Self {
        Self {
            name,
            tokens: Rc::new(RefCell::new(token)),
            capacity,
            span,
            place_type,
        }
    }
}

impl std::fmt::Display for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct Transition {
    pub name: String,
    pub weight: u32,
    shape: Shape,
    pub transition_type: ControlType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ControlType {
    Start(InstanceId),
    Goto,
    Switch,
    Return(InstanceId),
    Drop(DropType),
    Assert,

    UnsafeRead(NodeIndex, String, usize, String),
    UnsafeWrite(NodeIndex, String, usize, String),

    Call(CallType),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallType {
    Lock(NodeIndex),
    RwLockRead(NodeIndex),
    RwLockWrite(NodeIndex),
    Notify(NodeIndex),
    Wait,

    AtomicLoad(AliasId, AtomicOrdering, String, InstanceId),
    AtomicStore(AliasId, AtomicOrdering, String, InstanceId),
    AtomicCmpXchg(AliasId, AtomicOrdering, AtomicOrdering, String, InstanceId),

    Spawn(String),
    Join(String),

    Function,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DropType {
    Unlock(NodeIndex),
    DropRead(NodeIndex),
    DropWrite(NodeIndex),
    Basic,
}

impl Transition {
    pub fn new(name: String, transition_type: ControlType) -> Self {
        Self {
            name,
            transition_type,
            weight: 1,
            shape: Shape::Box,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PetriNetNode {
    P(Place),
    T(Transition),
}

impl std::fmt::Display for PetriNetNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PetriNetNode::P(place) => write!(f, "{}", place.name),
            PetriNetNode::T(transition) => write!(f, "{}", transition.name),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PetriNetEdge {
    pub label: u8,
}

impl std::fmt::Display for PetriNetEdge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marking {
    marks: HashMap<NodeIndex, usize>,
}

impl Hash for Marking {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for (key, value) in &self.marks {
            key.hash(state);
            value.hash(state);
        }
    }
}

#[derive(Error, Debug)]
pub enum PetriNetError {
    #[error("Invalid Petri net structure: Transition '{transition_name}' has a Transition {connection_type}")]
    InvalidTransitionConnection {
        transition_name: String,
        connection_type: &'static str,
    },

    #[error("Invalid Petri net structure: Place '{place_name}' has a Place {connection_type}")]
    InvalidPlaceConnection {
        place_name: String,
        connection_type: &'static str,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum CollectorType {
    Blocking,
    Atomic,
    Unsafe,
}

impl CollectorType {
    pub fn is_enabled(&self, config: &NetConfig) -> bool {
        match self {
            CollectorType::Blocking => config.enable_blocking,
            CollectorType::Atomic => config.enable_atomic,
            CollectorType::Unsafe => config.enable_unsafe,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetConfig {
    pub enable_blocking: bool,
    pub enable_atomic: bool,
    pub enable_unsafe: bool,
}

impl NetConfig {
    pub fn new(enable_blocking: bool, enable_atomic: bool, enable_unsafe: bool) -> Self {
        Self {
            enable_blocking,
            enable_atomic,
            enable_unsafe,
        }
    }

    pub fn all_enabled() -> Self {
        Self::new(true, true, true)
    }

    pub fn none_enabled() -> Self {
        Self::new(false, false, false)
    }
}

pub struct KeyApiRegex {
    pub thread_spawn: Regex,
    pub thread_join: Regex,
    pub scope_spwan: Regex,
    pub scope_join: Regex,
    pub condvar_notify: Regex,
    pub condvar_wait: Regex,

    pub channel_send: Regex,
    pub channel_recv: Regex,

    pub atomic_load: Regex,
    pub atomic_store: Regex,
}

impl KeyApiRegex {
    pub fn new() -> Self {
        Self {
            thread_spawn: Regex::new(r"std::thread[:a-zA-Z0-9_#\{\}]*::spawn").unwrap(),
            thread_join: Regex::new(r"std::thread[:a-zA-Z0-9_#\{\}]*::join").unwrap(),
            scope_spwan: Regex::new(r"std::thread::scoped[:a-zA-Z0-9_#\{\}]*::spawn").unwrap(),
            scope_join: Regex::new(r"std::thread::scoped[:a-zA-Z0-9_#\{\}]*::join").unwrap(),
            condvar_notify: Regex::new(r"condvar[:a-zA-Z0-9_#\{\}]*::notify").unwrap(),
            condvar_wait: Regex::new(r"condvar[:a-zA-Z0-9_#\{\}]*::wait").unwrap(),
            channel_send: Regex::new(r"mpsc[:a-zA-Z0-9_#\{\}]*::send").unwrap(),
            channel_recv: Regex::new(r"mpsc[:a-zA-Z0-9_#\{\}]*::recv").unwrap(),
            atomic_load: Regex::new(r"atomic[:a-zA-Z0-9]*::load").unwrap(),
            atomic_store: Regex::new(r"atomic[:a-zA-Z0-9]*::store").unwrap(),
        }
    }
}
