//! Group Petri-net places by logical function and extract per-function control-flow edges.
use std::collections::{BTreeMap, BTreeSet};

use crate::cir::naming::extract_function_name;
use crate::net::core::Net;
use crate::net::ids::{PlaceId, TransitionId};
use crate::net::structure::PlaceType;

#[derive(Debug, Clone)]
pub struct FunctionGroup {
    pub name: String,
    pub places: BTreeSet<PlaceId>,
    pub start: Option<PlaceId>,
    pub end: Option<PlaceId>,
}

pub fn group_functions(net: &Net) -> BTreeMap<String, FunctionGroup> {
    let mut map: BTreeMap<String, FunctionGroup> = BTreeMap::new();
    for (pid, place) in net.places.iter_enumerated() {
        let Some(fname) = extract_function_name(&place.name) else {
            continue;
        };
        let entry = map.entry(fname.clone()).or_insert_with(|| FunctionGroup {
            name: fname,
            places: BTreeSet::new(),
            start: None,
            end: None,
        });
        entry.places.insert(pid);
        match place.place_type {
            PlaceType::FunctionStart => entry.start = Some(pid),
            PlaceType::FunctionEnd => entry.end = Some(pid),
            _ => {}
        }
    }
    map
}

/// Parse `bb` index from `fn_foo_bb12` style names.
pub fn bb_index_from_place_name(name: &str) -> Option<usize> {
    let i = name.rfind("_bb")?;
    let rest = &name[i + 3..];
    rest.parse().ok()
}

/// Local CFG edges: control-flow places only (non-Resources), both ends in `f_places`.
pub fn local_edges_for_function(net: &Net, f_places: &BTreeSet<PlaceId>) -> Vec<(PlaceId, TransitionId, PlaceId)> {
    let mut out = Vec::new();
    for (tid, _t) in net.transitions.iter_enumerated() {
        let mut in_cf: Vec<PlaceId> = Vec::new();
        let mut out_cf: Vec<PlaceId> = Vec::new();
        for &p in f_places {
            if let Some(pl) = net.places.get(p) {
                if pl.place_type == PlaceType::Resources {
                    continue;
                }
            }
            if *net.pre.get(p, tid) > 0 {
                in_cf.push(p);
            }
            if *net.post.get(p, tid) > 0 {
                out_cf.push(p);
            }
        }
        if in_cf.is_empty() || out_cf.is_empty() {
            continue;
        }
        for &src in &in_cf {
            for &dst in &out_cf {
                out.push((src, tid, dst));
            }
        }
    }
    out
}
