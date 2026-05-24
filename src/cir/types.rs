//! CIR (Concurrency Intermediate Representation) data structures for YAML interchange.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Root artifact written to `cir.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CirArtifact {
    pub resources: BTreeMap<String, CirResource>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(default)]
    pub protection: BTreeMap<String, Vec<String>>,
    pub functions: BTreeMap<String, CirFunction>,
    pub goals: Vec<BusinessGoal>,
    pub entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub anchor_map: Option<AnchorMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CirResource {
    pub kind: ResourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paired_with: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "PascalCase")]
pub enum ResourceKind {
    Mutex,
    RwLock,
    Condvar,
    Var,
    Atomic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CirFunction {
    pub kind: FunctionKind,
    pub body: Vec<CirStatement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FunctionKind {
    Normal,
    Async,
    Closure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CirStatement {
    pub sid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<CirOp>,
    pub transfer: CirTransfer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
    /// Basic block index (internal ordering; omitted from YAML when absent).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub bb_index: Option<usize>,
}

/// Single-key operation maps (e.g. `{ lock: m0 }`, `{ call: foo }`).
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
        wait: WaitPayload,
    },
    NotifyOne {
        notify_one: String,
    },
    NotifyAll {
        notify_all: String,
    },
    Load {
        load: String,
    },
    Store {
        store: StorePayload,
    },
    Cas {
        cas: CasPayload,
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
pub struct WaitPayload {
    pub cv: String,
    pub mutex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorePayload {
    pub var: String,
    pub val: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CasPayload {
    pub var: String,
    pub expected: String,
    pub new: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum CirTransfer {
    Next {
        next: String,
    },
    Branch {
        branch: BranchPayload,
    },
    Done {
        done: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BranchPayload {
    pub cond: String,
    pub if_true: String,
    pub if_false: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BusinessGoal {
    pub id: String,
    pub desc: String,
    pub marking: BTreeMap<String, u64>,
}

/// Traceability to the Petri net (and resources). Serialized when present.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AnchorMap {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resource_to_places: BTreeMap<String, Vec<usize>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sid_to_place: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub place_to_sid: BTreeMap<usize, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sid_to_transition: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub transition_to_sid: BTreeMap<usize, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resource_id_to_name: BTreeMap<usize, String>,
}

impl CirTransfer {
    pub fn next_sid(s: impl Into<String>) -> Self {
        CirTransfer::Next { next: s.into() }
    }

    pub fn done() -> Self {
        CirTransfer::Done { done: true }
    }
}
