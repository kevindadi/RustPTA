//! Map Petri-net `TransitionType` labels (from MIR translation) to CIR ops — **not** from graph structure.
use crate::cir::resource_table::ResourceTable;
use crate::cir::types::{CasPayload, CirOp, StorePayload, WaitPayload};
use crate::net::structure::TransitionType;

/// Map a transition **label** to a CIR op. `held_locks` is only used for `Drop` without rid.
pub fn transition_to_cir_op(
    tt: &TransitionType,
    table: &ResourceTable,
    held_locks: &mut Vec<String>,
) -> Option<CirOp> {
    match tt {
        TransitionType::Lock(rid) => {
            let name = table.name_for_mutex_rid(*rid)?;
            held_locks.push(name.clone());
            Some(CirOp::Lock { lock: name })
        }
        TransitionType::Unlock(rid) => {
            let name = table.name_for_mutex_rid(*rid)?;
            held_locks.retain(|x| x != &name);
            Some(CirOp::Drop { drop: name })
        }
        TransitionType::Drop => {
            let name = held_locks.pop().unwrap_or_else(|| "unknown".into());
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
            cas: CasPayload {
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
        TransitionType::AsyncDone { .. } => None,
        _ => None,
    }
}
