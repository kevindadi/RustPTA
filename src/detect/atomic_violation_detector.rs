use std::collections::{BTreeMap, HashSet};

use crate::concurrency::atomic::AtomicOrdering;
use crate::memory::pointsto::AliasId;
use crate::net::core::Net;
use crate::net::ids::{PlaceId, TransitionId};
use crate::net::index_vec::{Idx, IndexVec};
use crate::net::structure::{Marking, Transition, TransitionType};

#[derive(Debug, Clone)]
pub enum AtomicEvent {
    Load {
        alias: AliasId,
        ordering: AtomicOrdering,
        span: String,
        tid: usize,
    },
    Store {
        alias: AliasId,
        ordering: AtomicOrdering,
        span: String,
        tid: usize,
    },
}

pub fn parse_atomic_event(tr: &Transition) -> Option<AtomicEvent> {
    match &tr.transition_type {
        TransitionType::AtomicLoad(alias, order, span, tid) => Some(AtomicEvent::Load {
            alias: *alias,
            ordering: *order,
            span: span.clone(),
            tid: *tid,
        }),
        TransitionType::AtomicStore(alias, order, span, tid) => Some(AtomicEvent::Store {
            alias: *alias,
            ordering: *order,
            span: span.clone(),
            tid: *tid,
        }),
        TransitionType::AtomicCmpXchg(alias, success, _failure, span, tid) => {
            Some(AtomicEvent::Store {
                alias: *alias,
                ordering: *success,
                span: span.clone(),
                tid: *tid,
            })
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Witness {
    pub alias: AliasId,
    pub tid_i: usize,
    pub tid_j: usize,
    pub trace_slice: Vec<TransitionId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct AliasKey {
    instance: usize,
    local: usize,
}

impl AliasKey {
    fn new(alias: AliasId) -> Self {
        Self {
            instance: alias.instance_id.index(),
            local: alias.local.index(),
        }
    }
}

type LoadKey = (usize, AliasKey);

#[derive(Clone)]
struct Frame {
    marking: Marking,
    trace: Vec<TransitionId>,
    pending_load: BTreeMap<LoadKey, usize>,
    intruder_of: BTreeMap<LoadKey, usize>,
}

impl Frame {
    fn new(marking: Marking) -> Self {
        Self {
            marking,
            trace: Vec::new(),
            pending_load: BTreeMap::new(),
            intruder_of: BTreeMap::new(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct StateFingerprint {
    marking: Vec<u64>,
    pending: Vec<(LoadKey, usize)>,
}

impl StateFingerprint {
    fn from_frame(net: &Net, frame: &Frame) -> Self {
        let marking = marking_key(net, &frame.marking);
        let pending = frame
            .pending_load
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect::<Vec<_>>();
        Self { marking, pending }
    }
}

pub fn detect_atomicity_violations(
    net: &Net,
    init: &Marking,
    max_states: usize,
    max_depth: usize,
) -> Vec<Witness> {
    if max_states == 0 {
        return Vec::new();
    }

    let mut witnesses = Vec::new();
    let mut witness_set: HashSet<Witness> = HashSet::new();
    let mut visited: HashSet<StateFingerprint> = HashSet::new();
    let mut scheduled: HashSet<StateFingerprint> = HashSet::new();

    let initial_frame = Frame::new(init.clone());
    let initial_fp = StateFingerprint::from_frame(net, &initial_frame);
    scheduled.insert(initial_fp.clone());

    let mut stack = vec![initial_frame];

    while let Some(frame) = stack.pop() {
        let fingerprint = StateFingerprint::from_frame(net, &frame);
        scheduled.remove(&fingerprint);

        if visited.contains(&fingerprint) {
            continue;
        }
        if visited.len() >= max_states {
            break;
        }

        visited.insert(fingerprint);

        if visited.len() >= max_states {
            break;
        }
        if frame.trace.len() >= max_depth {
            continue;
        }

        let mut enabled = net.enabled_transitions(&frame.marking);
        enabled.sort_by_key(|tid| tid.index());

        for transition_id in enabled {
            if visited.len() + scheduled.len() >= max_states {
                break;
            }
            if frame.trace.len() + 1 > max_depth {
                continue;
            }

            let fired = match net.fire_transition(&frame.marking, transition_id) {
                Ok(next) => next,
                Err(_) => continue,
            };

            let mut next_trace = frame.trace.clone();
            next_trace.push(transition_id);

            let mut next_pending = frame.pending_load.clone();
            let mut next_intruder = frame.intruder_of.clone();

            if let Some(event) = parse_atomic_event(&net.transitions[transition_id]) {
                let event_index = next_trace.len() - 1;
                match event {
                    AtomicEvent::Load { alias, tid, .. } => {
                        let key = (tid, AliasKey::new(alias));
                        next_pending.insert(key, event_index);
                        next_intruder.remove(&key);
                    }
                    AtomicEvent::Store { alias, tid, .. } => {
                        let alias_key = AliasKey::new(alias);
                        let affected_loads: Vec<LoadKey> = next_pending
                            .keys()
                            .filter(|(other_tid, other_key)| {
                                *other_tid != tid && *other_key == alias_key
                            })
                            .copied()
                            .collect();
                        for load_key in affected_loads {
                            next_intruder.insert(load_key, tid);
                        }

                        let load_key = (tid, alias_key);
                        if let Some(&load_idx) = next_pending.get(&load_key) {
                            if let Some(&intruder_tid) = next_intruder.get(&load_key) {
                                if load_idx <= event_index {
                                    let slice = next_trace[load_idx..=event_index].to_vec();
                                    let witness = Witness {
                                        alias,
                                        tid_i: tid,
                                        tid_j: intruder_tid,
                                        trace_slice: slice,
                                    };
                                    if witness_set.insert(witness.clone()) {
                                        witnesses.push(witness);
                                    }
                                }
                            }
                            next_pending.remove(&load_key);
                            next_intruder.remove(&load_key);
                        }
                    }
                }
            }

            let next_frame = Frame {
                marking: fired,
                trace: next_trace,
                pending_load: next_pending,
                intruder_of: next_intruder,
            };

            let next_fp = StateFingerprint::from_frame(net, &next_frame);
            if visited.contains(&next_fp) || scheduled.contains(&next_fp) {
                continue;
            }

            if visited.len() + scheduled.len() >= max_states {
                break;
            }

            scheduled.insert(next_fp);
            stack.push(next_frame);
        }
    }

    witnesses
}

pub fn print_witnesses(net: &Net, witnesses: &[Witness]) {
    if witnesses.is_empty() {
        println!("[atomic-violation] No violations found.");
        return;
    }

    println!("[atomic-violation] {} violation(s) found.", witnesses.len());

    for (idx, witness) in witnesses.iter().enumerate() {
        println!(
            "  #{} alias={:?} load_tid={} intruder_tid={} slice_len={}",
            idx,
            witness.alias,
            witness.tid_i,
            witness.tid_j,
            witness.trace_slice.len()
        );

        for (pos, transition_id) in witness.trace_slice.iter().enumerate() {
            let transition = &net.transitions[*transition_id];
            match &transition.transition_type {
                TransitionType::AtomicLoad(alias, ord, span, tid) => println!(
                    "    [{:03}] LOAD  a={:?} ord={:?} tid={} @{}",
                    pos, alias, ord, tid, span
                ),
                TransitionType::AtomicStore(alias, ord, span, tid) => println!(
                    "    [{:03}] STORE a={:?} ord={:?} tid={} @{}",
                    pos, alias, ord, tid, span
                ),
                TransitionType::AtomicCmpXchg(alias, succ, fail, span, tid) => println!(
                    "    [{:03}] CAS   a={:?} succ={:?} fail={:?} tid={} @{}",
                    pos, alias, succ, fail, tid, span
                ),
                other => println!("    [{:03}] {:?} ({})", pos, other, transition.name),
            }
        }
    }
}

pub fn marking_from_places(net: &Net) -> Marking {
    let tokens: Vec<u64> = net
        .places
        .iter_enumerated()
        .map(|(_, place)| place.tokens)
        .collect();
    Marking(IndexVec::<PlaceId, u64>::from_vec(tokens))
}

pub fn marking_key(net: &Net, marking: &Marking) -> Vec<u64> {
    net.places
        .iter_enumerated()
        .map(|(place_id, _)| marking.tokens(place_id))
        .collect()
}

pub fn run_atomic_violation_check(net: &Net) {
    let init = marking_from_places(net);
    let witnesses = detect_atomicity_violations(net, &init, 200_000, 10_000);
    print_witnesses(net, &witnesses);
}
