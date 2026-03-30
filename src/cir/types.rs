//! Native LLM-oriented CIR artifact (YAML-serializable). Not identical to `ceir::ast::Program`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CirArtifact {
    pub resources: BTreeMap<String, CirResource>,
    pub protection: BTreeMap<String, Vec<String>>,
    pub functions: BTreeMap<String, CirFunction>,
    pub goals: Vec<BusinessGoal>,
    pub entry: String,
    #[serde(default)]
    pub anchor_map: AnchorMap,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CirResource {
    pub kind: ResourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paired_with: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permits: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub var_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub init: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    Mutex,
    RwLock,
    Condvar,
    Semaphore,
    Channel,
    Var,
    Atomic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CirFunction {
    pub kind: FunctionKind,
    pub body: Vec<CirStatement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FunctionKind {
    Normal,
    Async,
    Closure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CirStatement {
    pub sid: String,
    pub op: CirOp,
    pub transfer: CirTransfer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum CirOp {
    Lock {
        lock: String,
    },
    Drop {
        drop: String,
    },
    ReadLock {
        read_lock: String,
    },
    WriteLock {
        write_lock: String,
    },
    Wait {
        wait: WaitOp,
    },
    NotifyOne {
        notify_one: String,
    },
    NotifyAll {
        notify_all: String,
    },
    Acquire {
        acquire: String,
    },
    Release {
        release: String,
    },
    Send {
        send: String,
    },
    Recv {
        recv: String,
    },
    Read {
        read: String,
    },
    Write {
        write: WriteOp,
    },
    Load {
        load: String,
    },
    Store {
        store: StoreOp,
    },
    Cas {
        cas: CasOp,
    },
    Spawn {
        spawn: String,
    },
    Join {
        join: String,
    },
    Call {
        call: String,
    },
    Return,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaitOp {
    pub cv: String,
    pub mutex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WriteOp {
    pub var: String,
    pub val: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoreOp {
    pub var: String,
    pub val: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CasOp {
    pub var: String,
    pub expected: String,
    pub new: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BranchTransfer {
    pub cond: String,
    pub if_true: String,
    pub if_false: String,
}

/// YAML: `{ next: sid }`, `{ branch: { cond, if_true, if_false } }`, or `{ done: true }`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum CirTransfer {
    Next { next: String },
    Branch { branch: BranchTransfer },
    Done { done: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BusinessGoal {
    pub id: String,
    pub desc: String,
    pub marking: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AnchorMap {
    pub resource_to_places: BTreeMap<String, Vec<usize>>,
    pub sid_to_place: BTreeMap<String, usize>,
    pub place_to_sid: BTreeMap<usize, String>,
    pub sid_to_transition: BTreeMap<String, usize>,
    pub transition_to_sid: BTreeMap<usize, String>,
    pub resource_id_to_name: BTreeMap<usize, String>,
}

impl CirOp {
    pub fn label(&self) -> String {
        match self {
            CirOp::Lock { .. } => "lock".into(),
            CirOp::Drop { .. } => "drop".into(),
            CirOp::ReadLock { .. } => "read_lock".into(),
            CirOp::WriteLock { .. } => "write_lock".into(),
            CirOp::Wait { .. } => "wait".into(),
            CirOp::NotifyOne { .. } => "notify_one".into(),
            CirOp::NotifyAll { .. } => "notify_all".into(),
            CirOp::Acquire { .. } => "acquire".into(),
            CirOp::Release { .. } => "release".into(),
            CirOp::Send { .. } => "send".into(),
            CirOp::Recv { .. } => "recv".into(),
            CirOp::Read { .. } => "read".into(),
            CirOp::Write { .. } => "write".into(),
            CirOp::Load { .. } => "load".into(),
            CirOp::Store { .. } => "store".into(),
            CirOp::Cas { .. } => "cas".into(),
            CirOp::Spawn { .. } => "spawn".into(),
            CirOp::Join { .. } => "join".into(),
            CirOp::Call { .. } => "call".into(),
            CirOp::Return => "return".into(),
        }
    }
}
