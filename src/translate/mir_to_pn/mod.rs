//! MIR 到 Petri 网转换主模块

mod async_control;
mod bb_graph;
mod calls;
mod closure;
mod concurrency;
mod drop_unsafe;
mod terminator;
mod thread_control;

use super::async_context::AsyncTranslateContext;
use super::callgraph::{CallGraph, InstanceId};
use bb_graph::BasicBlockGraph;
#[cfg(feature = "atomic-violation")]
use bb_graph::SegState;
use crate::{
    concurrency::blocking::LockGuardMap,
    memory::pointsto::{AliasAnalysis, AliasId},
    net::{Net, PlaceId, TransitionId},
    translate::structure::{FunctionRegistry, KeyApiRegex, ResourceRegistry},
};
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{
    BasicBlock, BasicBlockData, Statement, StatementKind,
    TerminatorKind, visit::Visitor,
};
use rustc_middle::{
    mir::{Body, Terminator},
    ty::{Instance, TyCtxt},
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub struct BodyToPetriNet<'translate, 'analysis, 'tcx> {
    instance_id: InstanceId,
    instance: &'translate Instance<'tcx>,
    body: &'translate Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    callgraph: &'translate CallGraph<'tcx>,
    pub net: &'translate mut Net,
    alias: &'translate mut RefCell<AliasAnalysis<'analysis, 'tcx>>,
    pub lockguards: Arc<LockGuardMap<'tcx>>,
    functions: &'translate FunctionRegistry,
    resources: &'translate ResourceRegistry,
    bb_graph: BasicBlockGraph,
    pub exclude_bb: HashSet<usize>,
    return_transition: TransitionId,
    entry_exit: (PlaceId, PlaceId),
    key_api_regex: &'translate KeyApiRegex,
    async_ctx: &'translate mut AsyncTranslateContext,
    alias_unknown_policy: crate::config::AliasUnknownPolicy,
    #[cfg(feature = "atomic-violation")]
    seg: SegState,
}

impl<'translate, 'analysis, 'tcx> BodyToPetriNet<'translate, 'analysis, 'tcx> {
    fn functions_map(&self) -> &HashMap<DefId, (PlaceId, PlaceId)> {
        self.functions.counter()
    }

    /// 按 join_id 从 spawn_calls 中 alias 匹配，返回可能对应的 spawn callee DefIds.
    fn get_matching_spawn_callees(&mut self, join_id: AliasId) -> Vec<DefId> {
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
        net: &'translate mut Net,
        alias: &'translate mut RefCell<AliasAnalysis<'analysis, 'tcx>>,
        lockguards: Arc<LockGuardMap<'tcx>>,
        functions: &'translate FunctionRegistry,
        resources: &'translate ResourceRegistry,
        entry_exit: (PlaceId, PlaceId),
        key_api_regex: &'translate KeyApiRegex,
        async_ctx: &'translate mut AsyncTranslateContext,
        alias_unknown_policy: crate::config::AliasUnknownPolicy,
    ) -> Self {
        #[allow(unused_mut)]
        let mut s = Self {
            instance_id,
            instance,
            body,
            tcx,
            callgraph,
            net,
            alias,
            lockguards,
            functions,
            resources,
            bb_graph: BasicBlockGraph::new(),
            exclude_bb: HashSet::new(),
            return_transition: TransitionId::new(0),
            entry_exit,
            key_api_regex,
            async_ctx,
            alias_unknown_policy,
            #[cfg(feature = "atomic-violation")]
            seg: SegState::default(),
        };

        #[cfg(feature = "atomic-violation")]
        {
            let tid = s.instance_id.index();
            s.seg.seg_index.insert(tid, 0);
            let seg_place = s.ensure_seg_place(tid, 0);
            if let Some(place) = s.net.get_place_mut(seg_place) {
                place.tokens = 1;
                if place.capacity < 1 {
                    place.capacity = 1;
                }
            }
        }

        s
    }

    pub fn translate(&mut self) {
        self.visit_body(self.body);
    }

    fn handle_terminator(
        &mut self,
        term: &Terminator<'tcx>,
        bb_idx: BasicBlock,
        name: &str,
        bb: &BasicBlockData<'tcx>,
    ) {
        match &term.kind {
            TerminatorKind::Goto { target } => self.handle_goto(bb_idx, target, name),
            TerminatorKind::SwitchInt { targets, .. } => self.handle_switch(bb_idx, targets, name),
            TerminatorKind::Return => self.handle_return(bb_idx, name),
            TerminatorKind::Assert { target, .. } => {
                self.handle_assert(bb_idx, target, name);
            }
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
            TerminatorKind::FalseEdge { real_target, .. } => {
                self.handle_fallthrough(bb_idx, real_target, name, "false_edge");
            }
            TerminatorKind::FalseUnwind { real_target, .. } => {
                self.handle_fallthrough(bb_idx, real_target, name, "false_unwind");
            }
            TerminatorKind::Yield { resume, .. } => {
                self.handle_fallthrough(bb_idx, resume, name, "yield");
            }
            TerminatorKind::InlineAsm {
                targets, unwind: _, ..
            } => {
                if let Some(target) = targets.first() {
                    self.handle_fallthrough(bb_idx, target, name, "inline_asm");
                } else {
                    self.handle_terminal_block(bb_idx, name, "inline_asm_noreturn");
                }
            }
            TerminatorKind::Unreachable => {
                self.handle_terminal_block(bb_idx, name, "unreachable");
            }
            TerminatorKind::UnwindResume => {
                self.handle_terminal_block(bb_idx, name, "unwind_resume");
            }
            TerminatorKind::UnwindTerminate(_) => {
                self.handle_terminal_block(bb_idx, name, "unwind_terminate");
            }
            TerminatorKind::CoroutineDrop => {
                self.handle_terminal_block(bb_idx, name, "coroutine_drop");
            }
            TerminatorKind::TailCall { .. } => {
                self.handle_terminal_block(bb_idx, name, "tail_call");
            }
        }
    }

    fn visit_statement_body(&mut self, statement: &Statement<'tcx>, bb_idx: BasicBlock) {
        let span_str = format!("{:?}", statement.source_info.span);
        if let StatementKind::Assign(box (dest, rvalue)) = &statement.kind {
            let fn_name = self.tcx.def_path_str(self.instance.def_id());

            self.process_rvalue_reads(rvalue, &fn_name, bb_idx, &span_str);

            self.process_place_writes(dest, &fn_name, bb_idx, &span_str);
        }
    }
}

impl<'translate, 'analysis, 'tcx> Visitor<'tcx> for BodyToPetriNet<'translate, 'analysis, 'tcx> {
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

        self.init_basic_block(body, &fn_name);

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

            if bb_idx.index() == 0 {
                self.handle_start_block(&fn_name, bb_idx, def_id);
            }

            if let Some(term) = &bb.terminator {
                self.handle_terminator(term, bb_idx, &fn_name, bb);
            }
        }
    }
}
