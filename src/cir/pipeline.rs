//! Merge Petri-net–extracted CIR with MIR-level direct calls and crate/file filtering.
use std::collections::{BTreeMap, BTreeSet};

use rustc_hir::def_id::DefId;
use rustc_middle::mir::{Body, TerminatorKind};
use rustc_middle::ty::{self, Instance, TyCtxt, TypingEnv};
use rustc_span::FileName;

use crate::cir::net_extract::CirExtractor;
use crate::cir::protection::infer_protection;
use crate::cir::types::{
    BusinessGoal, CirArtifact, CirFunction, CirOp, CirStatement, CirTransfer, FunctionKind,
};
use crate::options::{CrateNameList, Options};
use crate::translate::callgraph::{CallGraph, CallGraphNode};
use crate::translate::petri_net::PetriNet;
use crate::util::format_name;

/// Build CIR from the same inputs as the Petri net: **net structure + callgraph + MIR calls**.
pub fn build_cir_from_petri_net<'a, 'tcx>(
    pn: &'a PetriNet<'a, 'tcx>,
) -> Result<CirArtifact, Vec<crate::cir::net_extract::ExtractionError>> {
    let mut artifact = CirExtractor::new(&pn.net).extract()?;
    merge_calls_and_stubs(pn.tcx(), pn.options(), pn.callgraph(), &mut artifact);
    Ok(artifact)
}

/// Build `cir.yaml` artifact from **MIR walk** (`PetriNet::cir_*` filled during `construct`), not from net topology.
pub fn build_cir_artifact_from_mir_emission(pn: &PetriNet<'_, '_>) -> CirArtifact {
    let resources = pn.cir_resource_table.to_cir_resources_map();
    let mut artifact = CirArtifact {
        resources,
        protection: BTreeMap::new(),
        goals: Vec::new(),
        entry: pn
            .tcx()
            .entry_fn(())
            .map(|(def_id, _)| format_name(def_id))
            .unwrap_or_default(),
        anchor_map: None,
        functions: pn.cir_functions.clone(),
    };

    merge_calls_and_stubs(pn.tcx(), pn.options(), pn.callgraph(), &mut artifact);

    for (_name, cf) in &artifact.functions {
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

    let mut seen = BTreeSet::new();
    let mut g = 0u32;
    for name in &pn.cir_spawn_targets {
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

    artifact
}

pub fn merge_calls_and_stubs<'tcx>(
    tcx: TyCtxt<'tcx>,
    options: &Options,
    callgraph: &CallGraph<'tcx>,
    artifact: &mut CirArtifact,
) {
    for node_idx in callgraph.graph.node_indices() {
        let Some(node) = callgraph.graph.node_weight(node_idx) else {
            continue;
        };
        let instance = match node {
            CallGraphNode::WithBody(i) => i,
            CallGraphNode::WithoutBody(_) => continue,
        };
        let def_id = instance.def_id();
        if !def_in_scope(tcx, options, def_id) {
            continue;
        }
        if !tcx.is_mir_available(def_id) {
            continue;
        }
        let fname = format_name(def_id);
        if !artifact.functions.contains_key(&fname) {
            artifact.functions.insert(
                fname.clone(),
                stub_function(&fname, tcx, def_id),
            );
        }
        let body = tcx.instance_mir(instance.def);
        if body.source.promoted.is_some() {
            continue;
        }
        let calls = collect_direct_calls(tcx, instance, body, |c| def_in_scope(tcx, options, c));
        if calls.is_empty() {
            continue;
        }
        let prefix = crate::cir::naming::abbreviate_function_name(&fname);
        merge_calls_into_body(&mut artifact.functions.get_mut(&fname).unwrap().body, &calls, &prefix);
    }
}

fn stub_function(_fname: &str, tcx: TyCtxt<'_>, def_id: DefId) -> CirFunction {
    let kind = if tcx.is_closure_like(def_id) {
        FunctionKind::Closure
    } else {
        FunctionKind::Normal
    };
    CirFunction {
        kind,
        body: vec![CirStatement {
            sid: "ret".into(),
            op: None,
            transfer: CirTransfer::done(),
            span: None,
            bb_index: None,
        }],
    }
}

fn collect_direct_calls<'tcx>(
    tcx: TyCtxt<'tcx>,
    caller: &Instance<'tcx>,
    body: &Body<'tcx>,
    scope: impl Fn(DefId) -> bool,
) -> Vec<(usize, String)> {
    let typing_env = TypingEnv::post_analysis(tcx, caller.def_id());
    let mut out = Vec::new();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        let Some(term) = &data.terminator else {
            continue;
        };
        if let TerminatorKind::Call { func, .. } = &term.kind {
            let func_ty = caller.instantiate_mir_and_normalize_erasing_regions(
                tcx,
                typing_env,
                ty::EarlyBinder::bind(func.ty(body, tcx)),
            );
            if let ty::FnDef(def_id, _) = *func_ty.kind() {
                if scope(def_id) {
                    out.push((bb.index(), format_name(def_id)));
                }
            }
        }
    }
    out
}

fn merge_calls_into_body(body: &mut Vec<CirStatement>, calls: &[(usize, String)], prefix: &str) {
    if calls.is_empty() {
        return;
    }
    if let Some(last) = body.last() {
        if matches!(last.transfer, CirTransfer::Done { .. }) {
            body.pop();
        }
    }
    let mut by_bb: BTreeMap<usize, Vec<CirStatement>> = BTreeMap::new();
    for stmt in body.drain(..) {
        let bb = stmt.bb_index.unwrap_or(usize::MAX);
        by_bb.entry(bb).or_default().push(stmt);
    }
    let max_bb = calls.iter().map(|(b, _)| *b).max().unwrap_or(0);
    let max_sync_bb = by_bb.keys().filter(|&&k| k != usize::MAX).max().copied().unwrap_or(0);
    let max_k = max_bb.max(max_sync_bb);

    let mut merged: Vec<CirStatement> = Vec::new();
    let mut sid_counter = 1000u32;
    for bb in 0..=max_k {
        if let Some(mut v) = by_bb.remove(&bb) {
            merged.append(&mut v);
        }
        for (_, target) in calls.iter().filter(|(b, _)| *b == bb) {
            sid_counter += 1;
            let sid = format!("{prefix}_c{sid_counter}");
            merged.push(CirStatement {
                sid,
                op: Some(CirOp::Call {
                    call: target.clone(),
                }),
                transfer: CirTransfer::Next {
                    next: String::new(),
                },
                span: None,
                bb_index: Some(bb),
            });
        }
    }
    if let Some(mut rest) = by_bb.remove(&usize::MAX) {
        merged.append(&mut rest);
    }
    for v in by_bb.into_values() {
        merged.extend(v);
    }
    // Rewire `next` and append `ret`
    let ret_sid = "ret".to_string();
    for i in 0..merged.len() {
        let next = if i + 1 < merged.len() {
            merged[i + 1].sid.clone()
        } else {
            ret_sid.clone()
        };
        merged[i].transfer = CirTransfer::Next { next };
    }
    merged.push(CirStatement {
        sid: ret_sid,
        op: None,
        transfer: CirTransfer::done(),
        span: None,
        bb_index: None,
    });
    *body = merged;
}

/// Same rules as [`PetriNet::crate_filter_match`](crate::translate::petri_net::PetriNet::crate_filter_match) plus optional file path.
pub fn def_in_scope(tcx: TyCtxt<'_>, options: &Options, def_id: DefId) -> bool {
    if !def_id.is_local() {
        return false;
    }
    let name = format_name(def_id);
    let include = match &options.crate_filter {
        CrateNameList::White(list) if !list.is_empty() => list.iter().any(|c| name.starts_with(c)),
        _ => name.starts_with(&options.crate_name),
    };
    let exclude = match &options.crate_filter {
        CrateNameList::Black(list) if !list.is_empty() => list.iter().any(|c| name.starts_with(c)),
        _ => false,
    };
    if !include || exclude {
        return false;
    }
    if let Some(ref want) = options.input_file {
        let span = tcx.def_span(def_id);
        let file = tcx.sess.source_map().span_to_filename(span);
        let path_str: String = match &file {
            FileName::Real(p) => p
                .local_path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("{file:?}")),
            _ => format!("{file:?}"),
        };
        let want_s = want.to_string_lossy();
        if !path_str.contains(want_s.as_ref()) && !path_str.ends_with(&*want_s) {
            return false;
        }
    }
    true
}
