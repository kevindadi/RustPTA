//! 线程控制：spawn/join/scope/rayon

use super::BodyToPetriNet;
use crate::{
    memory::pointsto::AliasId,
    net::{Idx, Transition, TransitionId, TransitionType},
    translate::callgraph::{ThreadControlKind, classify_thread_control},
};
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{BasicBlock, Operand};
use rustc_span::source_map::Spanned;

impl<'translate, 'analysis, 'tcx> BodyToPetriNet<'translate, 'analysis, 'tcx> {
    pub(super) fn handle_thread_call(
        &mut self,
        callee_def_id: DefId,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        if let Some(kind) = classify_thread_control(
            self.tcx,
            callee_def_id,
            callee_func_name,
            self.key_api_regex,
        ) {
            match kind {
                ThreadControlKind::Spawn => {
                    self.handle_spawn(callee_func_name, args, target, bb_end);
                    return true;
                }
                ThreadControlKind::AsyncSpawn => {
                    self.handle_async_spawn(callee_func_name, args, target, bb_end);
                    return true;
                }
                ThreadControlKind::AsyncJoin => {
                    self.handle_async_join(callee_func_name, args, target, bb_end);
                    return true;
                }
                ThreadControlKind::ScopeSpawn => {
                    self.handle_scope_spawn(callee_func_name, bb_idx, args, target, bb_end);
                    return true;
                }
                ThreadControlKind::Join => {
                    self.handle_join(callee_func_name, args, target, bb_end);
                    return true;
                }
                ThreadControlKind::ScopeJoin => {
                    self.handle_scope_join(callee_func_name, args, target, bb_end);
                    return true;
                }
                ThreadControlKind::RayonJoin => {
                    self.handle_rayon_join(callee_func_name, bb_idx, args, target, bb_end, span);
                    return true;
                }
            }
        }
        false
    }

    pub(super) fn handle_scope_join(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) {
        let join_id = AliasId::from_place(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().as_ref(),
        );

        if let Some(spawn_calls) = self.callgraph.get_spawn_calls(self.instance.def_id()) {
            let matching_callees: Vec<_> = spawn_calls
                .iter()
                .filter_map(|(spawn_dest_id, callees)| {
                    let alias_kind = self
                        .alias
                        .borrow_mut()
                        .alias(join_id, *spawn_dest_id);

                    if alias_kind.may_alias(self.alias_unknown_policy) {
                        Some(callees.iter().copied())
                    } else {
                        None
                    }
                })
                .flatten()
                .collect();

            if matching_callees.is_empty() {
                log::error!(
                    "No matching spawn call found for join in {:?}",
                    self.instance.def_id()
                );
            }

            if let Some(transition) = self.net.get_transition_mut(bb_end) {
                transition.transition_type = TransitionType::Join(callee_func_name.to_string());
            }

            for spawn_def_id in matching_callees {
                if let Some((_, spawn_end)) = self.functions_map().get(&spawn_def_id).copied() {
                    self.net.add_input_arc(spawn_end, bb_end, 1);
                }
            }
        }
        self.connect_to_target(bb_end, target);
    }

    pub(super) fn handle_scope_spawn(
        &mut self,
        callee_func_name: &str,
        bb_idx: &BasicBlock,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) {
        if self.return_transition.index() == 0 {
            let bb_term_name = crate::transition_name!(callee_func_name, bb_idx, "return");
            let bb_term_transition =
                Transition::new_with_transition_type(bb_term_name, TransitionType::Function);
            self.return_transition = self.net.add_transition(bb_term_transition);
        }

        if let Some((closure_start, closure_end)) = self.resolve_closure_places_at(args, 1) {
            self.net.add_output_arc(closure_start, bb_end, 1);
            self.net.add_input_arc(closure_end, self.return_transition, 1);
        }
        self.connect_to_target(bb_end, target);
    }

    pub(super) fn handle_rayon_join(
        &mut self,
        callee_func_name: &str,
        bb_idx: &BasicBlock,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
        span: &str,
    ) {
        log::debug!("handle_rayon_join: {:?}", callee_func_name);
        let (_bb_wait, bb_join) = crate::add_wait_ret_subnet!(
            self,
            callee_func_name,
            bb_idx,
            "wait_closure",
            "join",
            TransitionType::Join(callee_func_name.to_string()),
            span.to_string(),
            bb_end
        );

        self.connect_to_target(bb_join, target);

        for (i, _) in args.iter().enumerate() {
            if let Some((closure_start, closure_end)) = self.resolve_closure_places_at(args, i) {
                self.net.add_output_arc(closure_start, bb_end, 1);
                self.net.add_input_arc(closure_end, bb_join, 1);
            }
        }
    }

    pub(super) fn handle_spawn(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) {
        if let Some(closure_start) = self.resolve_closure_start(args) {
            self.net.add_output_arc(closure_start, bb_end, 1);
        }

        if let Some(transition) = self.net.get_transition_mut(bb_end) {
            transition.transition_type = TransitionType::Spawn(callee_func_name.to_string());
        }
        self.connect_to_target(bb_end, target);
    }

    pub(super) fn handle_join(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) {
        let join_id = AliasId::from_place(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().as_ref(),
        );

        if let Some(spawn_calls) = self.callgraph.get_spawn_calls(self.instance.def_id()) {
            let matching_callees: Vec<_> = spawn_calls
                .iter()
                .filter_map(|(spawn_dest_id, callees)| {
                    let alias_kind = self
                        .alias
                        .borrow_mut()
                        .alias(join_id, *spawn_dest_id);

                    if alias_kind.may_alias(self.alias_unknown_policy) {
                        Some(callees.iter().copied())
                    } else {
                        None
                    }
                })
                .flatten()
                .collect();

            if matching_callees.is_empty() {
                log::error!(
                    "No matching spawn call found for join in {:?}",
                    self.instance.def_id()
                );
            }

            if let Some(transition) = self.net.get_transition_mut(bb_end) {
                transition.transition_type = TransitionType::Join(callee_func_name.to_string());
            }

            for spawn_def_id in matching_callees {
                if let Some((_, spawn_end)) = self.functions_map().get(&spawn_def_id).copied() {
                    self.net.add_input_arc(spawn_end, bb_end, 1);
                }
            }
        }

        self.connect_to_target(bb_end, target);
    }
}
