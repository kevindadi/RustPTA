//! Resource naming from Petri-net `TransitionType` (same resource ids as pointer analysis).
use std::collections::BTreeMap;

use crate::cir::types::{CirResource, ResourceKind};
use crate::memory::pointsto::AliasId;
use crate::net::structure::TransitionType;

#[derive(Debug, Clone)]
pub struct ResourceTable {
    /// Human-readable CIR name per mutex / rwlock / condvar / var resource id (`usize` from net).
    pub id_to_name: BTreeMap<usize, (String, crate::cir::types::ResourceKind)>,
    /// Atomic variables keyed by stable string (AliasId debug).
    pub atomic_key_to_name: BTreeMap<String, String>,
    /// Condvar resource id -> paired mutex resource id (when inferred).
    pub condvar_pairs: BTreeMap<usize, usize>,
    counters: ResourceCounters,
}

#[derive(Debug, Clone, Default)]
struct ResourceCounters {
    mutex: u32,
    rwlock: u32,
    condvar: u32,
    atomic: u32,
}

impl ResourceTable {
    pub fn from_transitions(
        transitions: impl Iterator<Item = TransitionType>,
    ) -> Self {
        let mut table = Self {
            id_to_name: BTreeMap::new(),
            atomic_key_to_name: BTreeMap::new(),
            condvar_pairs: BTreeMap::new(),
            counters: ResourceCounters::default(),
        };
        for tt in transitions {
            table.ingest_transition_type(&tt);
        }
        table
    }

    fn alloc_mutex(&mut self, rid: usize) -> String {
        if let Some((n, _)) = self.id_to_name.get(&rid) {
            return n.clone();
        }
        let name = format!("m{}", self.counters.mutex);
        self.counters.mutex += 1;
        self.id_to_name.insert(
            rid,
            (name.clone(), crate::cir::types::ResourceKind::Mutex),
        );
        name
    }

    fn alloc_rwlock(&mut self, rid: usize) -> String {
        if let Some((n, _)) = self.id_to_name.get(&rid) {
            return n.clone();
        }
        let name = format!("rw{}", self.counters.rwlock);
        self.counters.rwlock += 1;
        self.id_to_name.insert(
            rid,
            (name.clone(), crate::cir::types::ResourceKind::RwLock),
        );
        name
    }

    fn alloc_condvar(&mut self, rid: usize) -> String {
        if let Some((n, _)) = self.id_to_name.get(&rid) {
            return n.clone();
        }
        let name = format!("cv{}", self.counters.condvar);
        self.counters.condvar += 1;
        self.id_to_name.insert(
            rid,
            (name.clone(), crate::cir::types::ResourceKind::Condvar),
        );
        name
    }

    fn alloc_atomic(&mut self, alias: &AliasId, span_name: &str) -> String {
        let key = format!("{alias:?}");
        if let Some(n) = self.atomic_key_to_name.get(&key) {
            return n.clone();
        }
        let name = if !span_name.is_empty() {
            sanitize_name_hint(span_name)
        } else {
            let n = format!("a{}", self.counters.atomic);
            self.counters.atomic += 1;
            n
        };
        self.atomic_key_to_name.insert(key, name.clone());
        name
    }

    /// Register resource names for a transition label (same as Petri-net labeling).
    pub fn ingest(&mut self, tt: &TransitionType) {
        self.ingest_transition_type(tt);
    }

    fn ingest_transition_type(&mut self, tt: &TransitionType) {
        match tt {
            TransitionType::Lock(rid) | TransitionType::Unlock(rid) => {
                self.alloc_mutex(*rid);
            }
            TransitionType::RwLockRead(rid)
            | TransitionType::RwLockWrite(rid)
            | TransitionType::DropRead(rid)
            | TransitionType::DropWrite(rid) => {
                self.alloc_rwlock(*rid);
            }
            TransitionType::Notify(rid) => {
                self.alloc_condvar(*rid);
            }
            TransitionType::AtomicLoad(a, _, span, _)
            | TransitionType::AtomicStore(a, _, span, _)
            | TransitionType::AtomicCmpXchg(a, _, _, span, _) => {
                self.alloc_atomic(a, span);
            }
            _ => {}
        }
    }

    pub fn name_for_atomic(&self, alias: &AliasId, span: &str) -> String {
        let key = format!("{alias:?}");
        self.atomic_key_to_name
            .get(&key)
            .cloned()
            .unwrap_or_else(|| {
                if !span.is_empty() {
                    sanitize_name_hint(span)
                } else {
                    format!("a{}", alias.local.as_u32())
                }
            })
    }

    pub fn name_for_mutex_rid(&self, rid: usize) -> Option<String> {
        self.id_to_name.get(&rid).map(|(n, _)| n.clone())
    }

    pub fn name_for_rwlock_rid(&self, rid: usize) -> Option<String> {
        self.id_to_name.get(&rid).map(|(n, _)| n.clone())
    }

    pub fn name_for_condvar_rid(&self, rid: usize) -> Option<String> {
        self.id_to_name.get(&rid).map(|(n, _)| n.clone())
    }

    /// Build YAML `resources:` map (condvar `paired_with` when known).
    pub fn to_cir_resources_map(&self) -> BTreeMap<String, CirResource> {
        let mut m = BTreeMap::new();
        for (rid, (name, kind)) in &self.id_to_name {
            let paired = if *kind == ResourceKind::Condvar {
                self.condvar_pairs
                    .get(rid)
                    .and_then(|mrid| self.name_for_mutex_rid(*mrid))
            } else {
                None
            };
            m.insert(
                name.clone(),
                CirResource {
                    kind: kind.clone(),
                    paired_with: paired,
                    span: None,
                },
            );
        }
        for (_k, n) in &self.atomic_key_to_name {
            if !m.contains_key(n) {
                m.insert(
                    n.clone(),
                    CirResource {
                        kind: ResourceKind::Atomic,
                        paired_with: None,
                        span: None,
                    },
                );
            }
        }
        m
    }
}

fn sanitize_name_hint(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars().take(32) {
        if ch.is_alphanumeric() || ch == '_' {
            out.push(ch);
        }
    }
    if out.is_empty() {
        "v0".into()
    } else {
        out
    }
}
