//! 仅基于 **MIR 遍历** 发射 CIR（与 `mir_to_pn` 并列，逻辑有意重复，不修改 `mir_to_pn`）。
//!
//! 使用与 Petri 翻译相同的 `TransitionType` 标签语义，但**不**从 Petri 网拓扑读取 MIR 操作。

mod async_control;
mod calls;
mod cfg_utils;
mod closure;
mod concurrency;
mod drop;
mod thread_control;

use super::async_context::AsyncTranslateContext;
use super::callgraph::{CallGraph, InstanceId};
use crate::cir::mir_emitter::CirMirEmitter;
use crate::concurrency::blocking::LockGuardMap;
use crate::memory::pointsto::{AliasAnalysis, AliasId};
use crate::net::structure::TransitionType;
use crate::options::Options;
use crate::translate::structure::{FunctionRegistry, KeyApiRegex, ResourceRegistry};
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{
    BasicBlock, BasicBlockData, Local, Operand, StatementKind,
    TerminatorKind, visit::Visitor,
};
use rustc_middle::{
    mir::{Body, Terminator},
    ty::{Instance, TyCtxt},
};
use rustc_hash::FxHashSet;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

pub struct BodyToCir<'translate, 'analysis, 'tcx, 'a> {
    pub instance_id: InstanceId,
    pub instance: &'translate Instance<'tcx>,
    pub body: &'translate Body<'tcx>,
    pub tcx: TyCtxt<'tcx>,
    pub callgraph: &'translate CallGraph<'tcx>,
    pub alias: &'translate mut RefCell<AliasAnalysis<'analysis, 'tcx>>,
    pub lockguards: Arc<LockGuardMap<'tcx>>,
    pub functions: &'translate FunctionRegistry,
    pub resources: &'translate ResourceRegistry,
    pub key_api_regex: &'translate KeyApiRegex,
    pub async_ctx: &'translate mut AsyncTranslateContext,
    pub alias_unknown_policy: crate::config::AliasUnknownPolicy,
    pub break_cfg_cycles: bool,
    pub back_edges: FxHashSet<(BasicBlock, BasicBlock)>,
    pub ordered_spawn_ends: VecDeque<crate::net::PlaceId>,
    pub spawn_handle_end: HashMap<Local, crate::net::PlaceId>,
    pub local_ref_source: HashMap<Local, Local>,
    pub vec_alias_source: HashMap<Local, Local>,
    pub vec_spawn_ends: HashMap<Local, VecDeque<crate::net::PlaceId>>,
    pub iter_vec_source: HashMap<Local, Local>,
    pub option_vec_source: HashMap<Local, Local>,
    pub handle_vec_source: HashMap<Local, Local>,
    pub joinhandle_vec_locals: HashSet<Local>,
    pub(crate) emitter: &'translate mut CirMirEmitter<'a>,
    pub(crate) options: &'translate Options,
}

impl<'translate, 'analysis, 'tcx, 'a> BodyToCir<'translate, 'analysis, 'tcx, 'a> {
    pub fn functions_map(&self) -> &HashMap<DefId, (crate::net::PlaceId, crate::net::PlaceId)> {
        self.functions.counter()
    }

    pub fn is_back_edge(&self, src: BasicBlock, target: BasicBlock) -> bool {
        self.break_cfg_cycles && self.back_edges.contains(&(src, target))
    }

    pub fn get_matching_spawn_callees(&mut self, join_id: AliasId) -> Vec<DefId> {
        self.callgraph
            .get_spawn_calls(self.instance.def_id())
            .map(|spawn_calls| {
                spawn_calls
                    .iter()
                    .filter_map(|(spawn_dest_id, callees)| {
                        let alias_kind =
                            self.alias.borrow_mut().alias(join_id, *spawn_dest_id);
                        if alias_kind.may_alias(self.alias_unknown_policy) {
                            Some(callees.iter().copied())
                        } else {
                            None
                        }
                    })
                    .flatten()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    pub fn new(
        instance_id: InstanceId,
        instance: &'translate Instance<'tcx>,
        body: &'translate Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        callgraph: &'translate CallGraph<'tcx>,
        alias: &'translate mut RefCell<AliasAnalysis<'analysis, 'tcx>>,
        lockguards: Arc<LockGuardMap<'tcx>>,
        functions: &'translate FunctionRegistry,
        resources: &'translate ResourceRegistry,
        key_api_regex: &'translate KeyApiRegex,
        async_ctx: &'translate mut AsyncTranslateContext,
        alias_unknown_policy: crate::config::AliasUnknownPolicy,
        break_cfg_cycles: bool,
        emitter: &'translate mut CirMirEmitter<'a>,
        options: &'translate Options,
    ) -> Self {
        let joinhandle_vec_locals: HashSet<Local> = body
            .local_decls
            .iter_enumerated()
            .filter_map(|(local, decl)| {
                let ty_str = format!("{:?}", decl.ty);
                if ty_str.contains("Vec") && ty_str.contains("JoinHandle") {
                    Some(local)
                } else {
                    None
                }
            })
            .collect();

        Self {
            instance_id,
            instance,
            body,
            tcx,
            callgraph,
            alias,
            lockguards,
            functions,
            resources,
            key_api_regex,
            async_ctx,
            alias_unknown_policy,
            break_cfg_cycles,
            back_edges: FxHashSet::default(),
            ordered_spawn_ends: VecDeque::new(),
            spawn_handle_end: HashMap::new(),
            local_ref_source: HashMap::new(),
            vec_alias_source: HashMap::new(),
            vec_spawn_ends: HashMap::new(),
            iter_vec_source: HashMap::new(),
            option_vec_source: HashMap::new(),
            handle_vec_source: HashMap::new(),
            joinhandle_vec_locals,
            emitter,
            options,
        }
    }

    pub fn translate(&mut self) {
        self.visit_body(self.body);
    }

    pub(crate) fn emit_tt(&mut self, tt: &TransitionType, bb_idx: BasicBlock, span: &str) {
        let span_opt = if span.is_empty() {
            None
        } else {
            Some(span.to_string())
        };
        self.emitter.emit(tt, bb_idx.index(), span_opt);
    }

    pub(crate) fn emit_call_if_in_scope(&mut self, callee_def_id: DefId, bb_idx: BasicBlock) {
        use crate::cir::pipeline::def_in_scope;
        use crate::util::format_name;
        if !def_in_scope(self.tcx, self.options, callee_def_id) {
            return;
        }
        self.emitter
            .emit_call(&format_name(callee_def_id), bb_idx.index());
    }

    fn handle_terminator(
        &mut self,
        term: &Terminator<'tcx>,
        bb_idx: BasicBlock,
        name: &str,
        bb: &BasicBlockData<'tcx>,
    ) {
        match &term.kind {
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
                unwind,
                ..
            } => {
                self.handle_call(
                    bb_idx,
                    func,
                    args,
                    destination,
                    target,
                    name,
                    &format!("{:?}", term.source_info.span),
                    unwind,
                );
            }
            TerminatorKind::Drop { place, target, .. } => {
                self.handle_drop(&bb_idx, place, target, name, bb)
            }
            _ => {}
        }
    }

    fn visit_statement_body(&mut self, statement: &rustc_middle::mir::Statement<'tcx>, _bb_idx: BasicBlock) {
        if let StatementKind::Assign(box (dest, rvalue)) = &statement.kind {
            self.track_joinhandle_dataflow(dest.local, rvalue);
        }
    }

    fn track_joinhandle_dataflow(&mut self, dest: Local, rvalue: &rustc_middle::mir::Rvalue<'tcx>) {
        match rvalue {
            rustc_middle::mir::Rvalue::Ref(_, _, place) => {
                self.local_ref_source.insert(dest, place.local);
            }
            rustc_middle::mir::Rvalue::Use(op) => {
                if let Operand::Move(place) | Operand::Copy(place) = op {
                    let src = place.local;
                    if let Some(end) = self.spawn_handle_end.get(&src).copied() {
                        self.spawn_handle_end.insert(dest, end);
                    }
                    if let Some(vec_local) = self.iter_vec_source.get(&src).copied() {
                        self.iter_vec_source.insert(dest, vec_local);
                    }
                    if let Some(vec_local) = self.option_vec_source.get(&src).copied() {
                        self.option_vec_source.insert(dest, vec_local);
                    }
                    if let Some(vec_local) = self.handle_vec_source.get(&src).copied() {
                        self.handle_vec_source.insert(dest, vec_local);
                    }
                    let src_vec = self.resolve_vec_local(src);
                    if self.vec_spawn_ends.contains_key(&src_vec) {
                        self.vec_alias_source.insert(dest, src_vec);
                    }
                }
            }
            _ => {}
        }
    }
}

impl<'translate, 'analysis, 'tcx, 'a> Visitor<'tcx> for BodyToCir<'translate, 'analysis, 'tcx, 'a> {
    fn visit_body(&mut self, body: &Body<'tcx>) {
        let def_id = self.instance.def_id();
        let fn_name = self.tcx.def_path_str(def_id);

        if fn_name.contains("::deserialize")
            || fn_name.contains("::serialize")
            || fn_name.contains("::visit_seq")
            || fn_name.contains("::visit_map")
        {
            log::warn!("Skipping serialization function: {}", fn_name);
            return;
        }

        if self.break_cfg_cycles {
            self.back_edges = cfg_utils::compute_back_edges(body);
        }

        for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
            if bb.is_cleanup || bb.is_empty_unreachable() {
                continue;
            }

            for stmt in bb.statements.iter() {
                if let Some(ref term) = bb.terminator {
                    if let TerminatorKind::Assert { .. } = &term.kind {
                        break;
                    }
                }
                self.visit_statement_body(stmt, bb_idx);
            }

            if let Some(term) = &bb.terminator {
                self.handle_terminator(term, bb_idx, &fn_name, bb);
            }
        }
    }
}
