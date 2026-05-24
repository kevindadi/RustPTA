//! Build [`CirArtifact`](crate::cir::types::CirArtifact) from a Petri [`Net`](crate::net::core::Net) (read-only).
use std::collections::{BTreeMap, BTreeSet};

use crate::cir::function_grouper::{bb_index_from_place_name, group_functions, local_edges_for_function};
use crate::cir::naming::abbreviate_function_name;
use crate::cir::protection::infer_protection;
use crate::cir::resource_table::ResourceTable;
use crate::cir::types::{
    AnchorMap, BusinessGoal, CirArtifact, CirFunction, CirOp, CirResource, CirStatement, CirTransfer,
    FunctionKind, ResourceKind, StorePayload, WaitPayload,
};
use crate::net::core::Net;
use crate::net::ids::{PlaceId, TransitionId};
use crate::net::structure::{PlaceType, TransitionType};
use crate::net::Idx;

/// Read-only extractor over an existing Petri net.
pub struct CirExtractor<'a> {
    net: &'a Net,
}

#[derive(Debug, thiserror::Error)]
pub enum ExtractionError {
    #[error("no entry function found (no FunctionStart with tokens > 0)")]
    NoEntryFunction,
    #[error("condvar {0} has no paired mutex")]
    UnpairedCondvar(String),
    #[error("function {0} has no FunctionStart place")]
    NoFunctionStart(String),
}

impl<'a> CirExtractor<'a> {
    pub fn new(net: &'a Net) -> Self {
        Self { net }
    }

    pub fn extract(&self) -> Result<CirArtifact, Vec<ExtractionError>> {
        let mut errors = Vec::new();
        let tts: Vec<TransitionType> = self
            .net
            .transitions
            .iter()
            .map(|t| t.transition_type.clone())
            .collect();
        let mut table = ResourceTable::from_transitions(tts.into_iter());
        self.infer_condvar_pairing(&mut table, &mut errors);

        let groups = group_functions(self.net);
        let mut functions: BTreeMap<String, CirFunction> = BTreeMap::new();
        let mut anchor = AnchorMap::default();

        for (fname, group) in &groups {
            if group.start.is_none() {
                errors.push(ExtractionError::NoFunctionStart(fname.clone()));
                continue;
            }
            let kind = if fname.contains("closure") || fname.contains('{') {
                FunctionKind::Closure
            } else {
                FunctionKind::Normal
            };
            let body = self.walk_function(
                fname,
                group,
                &table,
                &mut anchor,
            );
            functions.insert(
                fname.clone(),
                CirFunction { kind, body },
            );
        }

        let resources = build_cir_resources(&table);
        for (rid, (name, _)) in &table.id_to_name {
            anchor.resource_id_to_name.insert(*rid, name.clone());
        }

        let mut artifact = CirArtifact {
            protection: BTreeMap::new(),
            goals: Vec::new(),
            entry: String::new(),
            anchor_map: Some(anchor),
            resources,
            functions: BTreeMap::new(),
        };

        // Protection per function
        for (_name, cf) in &functions {
            let prot = infer_protection(&cf.body, &artifact.resources);
            for (k, v) in prot {
                artifact
                    .protection
                    .entry(k)
                    .or_insert_with(Vec::new)
                    .extend(v);
            }
        }
        for v in artifact.protection.values_mut() {
            let s: BTreeSet<String> = v.iter().cloned().collect();
            *v = s.into_iter().collect();
        }

        artifact.functions = functions;

        // Goals: one per distinct spawn target
        let mut seen = BTreeSet::new();
        let mut g = 0u32;
        for t in self.net.transitions.iter() {
            if let TransitionType::Spawn(name) = &t.transition_type {
                if seen.insert(name.clone()) {
                    artifact.goals.push(BusinessGoal {
                        id: format!("G{g}"),
                        desc: format!("{name} completes"),
                        marking: {
                            let mut m = BTreeMap::new();
                            m.insert(format!("cp({name}, ret)"), 1);
                            m
                        },
                    });
                    g += 1;
                }
            }
        }

        // Entry
        let mut entry = None;
        for (_pid, place) in self.net.places.iter_enumerated() {
            if place.place_type == PlaceType::FunctionStart && place.tokens > 0 {
                if let Some(fname) = crate::cir::naming::extract_function_name(&place.name) {
                    entry = Some(fname);
                    break;
                }
            }
        }
        artifact.entry = match entry {
            Some(e) => e,
            None => {
                errors.push(ExtractionError::NoEntryFunction);
                String::new()
            }
        };

        if errors.is_empty() {
            Ok(artifact)
        } else {
            Err(errors)
        }
    }

    fn infer_condvar_pairing(&self, table: &mut ResourceTable, _errors: &mut Vec<ExtractionError>) {
        // For each Wait, find mutex resource place via post arcs; map condvar via Wait's pre/post with Notify rid elsewhere — simplified: pair Notify rid with mutex from nearest Lock on same thread is complex. Here: for each Wait transition, any Resources post place shared with an Unlock transition's post identifies released mutex rid.
        for (tid, t) in self.net.transitions.iter_enumerated() {
            if !matches!(t.transition_type, TransitionType::Wait) {
                continue;
            }
            let mut mutex_rid = None;
            for (pid, place) in self.net.places.iter_enumerated() {
                if place.place_type != PlaceType::Resources {
                    continue;
                }
                if *self.net.post.get(pid, tid) == 0 {
                    continue;
                }
                // This resource place receives token on Wait — mutex released into pool
                for (tid2, t2) in self.net.transitions.iter_enumerated() {
                    if let TransitionType::Unlock(rid) = t2.transition_type {
                        if *self.net.post.get(pid, tid2) > 0 || *self.net.pre.get(pid, tid2) > 0 {
                            mutex_rid = Some(rid);
                            break;
                        }
                    }
                }
                if mutex_rid.is_some() {
                    break;
                }
            }
            // Condvar id: from a Notify in same function — skip if not found
            let cv_rid = (|| {
                for (pid, place) in self.net.places.iter_enumerated() {
                    if place.place_type != PlaceType::Resources {
                        continue;
                    }
                    if *self.net.pre.get(pid, tid) > 0 || *self.net.post.get(pid, tid) > 0 {
                        for (tid2, t2) in self.net.transitions.iter_enumerated() {
                            if let TransitionType::Notify(r) = t2.transition_type {
                                if *self.net.pre.get(pid, tid2) > 0 || *self.net.post.get(pid, tid2) > 0 {
                                    return Some(r);
                                }
                            }
                        }
                    }
                }
                None
            })();
            if let (Some(cv), Some(m)) = (cv_rid, mutex_rid) {
                table.condvar_pairs.insert(cv, m);
            }
        }
    }

    fn walk_function(
        &self,
        fname: &str,
        group: &crate::cir::function_grouper::FunctionGroup,
        table: &ResourceTable,
        anchor: &mut AnchorMap,
    ) -> Vec<CirStatement> {
        let f_places = &group.places;
        let mut edges = local_edges_for_function(self.net, f_places);
        edges.sort_by_key(|(src, tid, _)| {
            let bb = bb_index_from_place_name(&self.net.places[*src].name).unwrap_or(0);
            (bb, tid.index())
        });
        let mut seen_t: BTreeSet<TransitionId> = BTreeSet::new();
        let mut walk_order: Vec<(PlaceId, TransitionId, PlaceId)> = Vec::new();
        for (src, tid, dst) in edges {
            if seen_t.insert(tid) {
                walk_order.push((src, tid, dst));
            }
        }

        let prefix = abbreviate_function_name(fname);
        let mut counter = 1u32;
        let mut body: Vec<CirStatement> = Vec::new();
        let mut held_locks: Vec<String> = Vec::new();
        for (src, tid, _dst) in walk_order {
            let t = &self.net.transitions[tid];
            let tt = &t.transition_type;
            let span = self.net.places[src].span.clone();
            let span_opt = if span.is_empty() {
                None
            } else {
                Some(span)
            };

            if matches!(
                tt,
                TransitionType::Goto
                    | TransitionType::Normal
                    | TransitionType::Assert
                    | TransitionType::Function
                    | TransitionType::Inhibitor
                    | TransitionType::Reset
            ) || matches!(tt, TransitionType::Start(_))
            {
                continue;
            }

            if matches!(
                tt,
                TransitionType::AsyncPoll { .. }
                    | TransitionType::AwaitReady { .. }
                    | TransitionType::AsyncAbort { .. }
            ) {
                continue;
            }

            if let TransitionType::Switch = tt {
                continue;
            }

            let op = map_transition(
                tt,
                table,
                &mut held_locks,
            );
            let Some(op) = op else { continue };

            let sid = format!("{prefix}_{counter}");
            counter += 1;

            anchor.sid_to_place.insert(sid.clone(), src.index());
            anchor.place_to_sid.insert(src.index(), sid.clone());
            anchor.sid_to_transition.insert(sid.clone(), tid.index());
            anchor.transition_to_sid.insert(tid.index(), sid.clone());

            let stmt = CirStatement {
                sid: sid.clone(),
                op: Some(op),
                transfer: CirTransfer::Next {
                    next: String::new(),
                },
                span: span_opt,
                bb_index: bb_index_from_place_name(&self.net.places[src].name),
            };
            body.push(stmt);
        }

        let ret_sid = "ret".to_string();
        // Wire Next transfers through `ret`
        for i in 0..body.len() {
            let next_sid = if i + 1 < body.len() {
                body[i + 1].sid.clone()
            } else {
                ret_sid.clone()
            };
            body[i].transfer = CirTransfer::Next { next: next_sid };
        }

        body.push(CirStatement {
            sid: ret_sid,
            op: None,
            transfer: CirTransfer::done(),
            span: None,
            bb_index: None,
        });

        body
    }
}

fn build_cir_resources(table: &ResourceTable) -> BTreeMap<String, CirResource> {
    let mut m = BTreeMap::new();
    for (rid, (name, kind)) in &table.id_to_name {
        let paired = if *kind == ResourceKind::Condvar {
            table
                .condvar_pairs
                .get(rid)
                .and_then(|mrid| table.name_for_mutex_rid(*mrid))
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
    for (_k, n) in &table.atomic_key_to_name {
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

fn map_transition(
    tt: &TransitionType,
    table: &ResourceTable,
    held: &mut Vec<String>,
) -> Option<CirOp> {
    match tt {
        TransitionType::Lock(rid) => {
            let name = table.name_for_mutex_rid(*rid)?;
            held.push(name.clone());
            Some(CirOp::Lock { lock: name })
        }
        TransitionType::Unlock(rid) => {
            let name = table.name_for_mutex_rid(*rid)?;
            held.retain(|x| x != &name);
            Some(CirOp::Drop { drop: name })
        }
        TransitionType::Drop => {
            let name = held.pop().unwrap_or_else(|| "unknown".into());
            Some(CirOp::Drop { drop: name })
        }
        TransitionType::RwLockRead(rid) => {
            let n = table.name_for_rwlock_rid(*rid)?;
            Some(CirOp::ReadLock { read_lock: n })
        }
        TransitionType::RwLockWrite(rid) => {
            let n = table.name_for_rwlock_rid(*rid)?;
            Some(CirOp::WriteLock { write_lock: n })
        }
        TransitionType::DropRead(rid) | TransitionType::DropWrite(rid) => {
            let n = table.name_for_rwlock_rid(*rid)?;
            Some(CirOp::Drop { drop: n })
        }
        TransitionType::Wait => {
            if let Some((cv_rid, m_rid)) = table.condvar_pairs.iter().next() {
                Some(CirOp::Wait {
                    wait: WaitPayload {
                        cv: table.name_for_condvar_rid(*cv_rid)?,
                        mutex: table.name_for_mutex_rid(*m_rid)?,
                    },
                })
            } else {
                Some(CirOp::Wait {
                    wait: WaitPayload {
                        cv: "cv0".into(),
                        mutex: "m0".into(),
                    },
                })
            }
        }
        TransitionType::Notify(rid) => {
            let n = table.name_for_condvar_rid(*rid)?;
            Some(CirOp::NotifyOne { notify_one: n })
        }
        TransitionType::AtomicLoad(a, _, _, _) => {
            let n = table.name_for_atomic(a, "");
            Some(CirOp::Load { load: n })
        }
        TransitionType::AtomicStore(a, _, span, _) => Some(CirOp::Store {
            store: StorePayload {
                var: table.name_for_atomic(a, span),
                val: "unknown".into(),
            },
        }),
        TransitionType::AtomicCmpXchg(a, _, _, span, _) => Some(CirOp::Cas {
            cas: crate::cir::types::CasPayload {
                var: table.name_for_atomic(a, span),
                expected: "unknown".into(),
                new: "unknown".into(),
            },
        }),
        TransitionType::Spawn(name) => Some(CirOp::Spawn {
            spawn: name.clone(),
        }),
        TransitionType::Join(name) => Some(CirOp::Join {
            join: name.clone(),
        }),
        TransitionType::Return(_) => None,
        TransitionType::AsyncSpawn { task_id } => Some(CirOp::Spawn {
            spawn: format!("task_{task_id}"),
        }),
        TransitionType::AsyncJoin { task_id } => Some(CirOp::Join {
            join: format!("task_{task_id}"),
        }),
        TransitionType::AwaitPending { task_id, event_id, .. } => {
            let ev = event_id.map(|e| format!("ev{e}")).unwrap_or_else(|| "ev".into());
            let mtx = format!("task_mutex_{task_id}");
            Some(CirOp::Wait {
                wait: WaitPayload {
                    cv: ev,
                    mutex: mtx,
                },
            })
        }
        TransitionType::AsyncWake { event_id, .. } => Some(CirOp::NotifyOne {
            notify_one: format!("ev{event_id}"),
        }),
        TransitionType::AsyncDone { .. } => Some(CirOp::Return),
        _ => None,
    }
}
