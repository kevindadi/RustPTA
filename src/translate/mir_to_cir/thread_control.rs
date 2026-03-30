//! 线程控制（与 `mir_to_pn/thread_control` 重复，不建网）

use super::BodyToCir;
use crate::{
    memory::pointsto::AliasId,
    net::structure::TransitionType,
    translate::callgraph::{ThreadControlKind, classify_thread_control},
};
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{BasicBlock, Local, Operand};
use rustc_span::source_map::Spanned;

impl<'translate, 'analysis, 'tcx, 'a> BodyToCir<'translate, 'analysis, 'tcx, 'a> {
    fn connect_join_from_recorded_order(&mut self) -> bool {
        self.ordered_spawn_ends.pop_front().is_some()
    }

    pub(super) fn resolve_vec_local(&self, local: Local) -> Local {
        self.vec_alias_source.get(&local).copied().unwrap_or(local)
    }

    pub(super) fn track_joinhandle_container_call(
        &mut self,
        callee_func_name: &str,
        args: &[Spanned<Operand<'tcx>>],
        destination: Local,
    ) {
        if callee_func_name.contains("::push") {
            let Some(vec_ref_local) = args.first().and_then(|a| a.node.place()).map(|p| p.local)
            else {
                return;
            };
            let vec_local = self
                .local_ref_source
                .get(&vec_ref_local)
                .copied()
                .unwrap_or(vec_ref_local);
            let vec_root = self.resolve_vec_local(vec_local);
            if !self.joinhandle_vec_locals.contains(&vec_root) {
                return;
            }
            let Some(handle_local) = args.get(1).and_then(|a| a.node.place()).map(|p| p.local)
            else {
                return;
            };
            self.vec_alias_source.insert(vec_local, vec_root);
            if let Some(spawn_end) = self.spawn_handle_end.get(&handle_local).copied() {
                self.vec_spawn_ends
                    .entry(vec_root)
                    .or_default()
                    .push_back(spawn_end);
            }
            return;
        }

        if callee_func_name.contains("into_iter") {
            let Some(src_local) = args.first().and_then(|a| a.node.place()).map(|p| p.local) else {
                return;
            };
            let vec_local = self.resolve_vec_local(src_local);
            if self.joinhandle_vec_locals.contains(&vec_local)
                || self.vec_spawn_ends.contains_key(&vec_local)
            {
                self.iter_vec_source.insert(destination, vec_local);
            }
            return;
        }

        if callee_func_name.contains("::next") {
            let Some(iter_ref_local) = args.first().and_then(|a| a.node.place()).map(|p| p.local)
            else {
                return;
            };
            let iter_local = self
                .local_ref_source
                .get(&iter_ref_local)
                .copied()
                .unwrap_or(iter_ref_local);
            if let Some(vec_local) = self.iter_vec_source.get(&iter_local).copied() {
                self.option_vec_source.insert(destination, vec_local);
            }
        }
    }

    pub(super) fn handle_thread_call(
        &mut self,
        callee_def_id: DefId,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        destination: Local,
        target: &Option<BasicBlock>,
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
                    self.handle_spawn(callee_func_name, args, destination, target, *bb_idx, span);
                    return true;
                }
                ThreadControlKind::AsyncSpawn => {
                    super::async_control::handle_async_spawn(
                        self, callee_func_name, args, target, *bb_idx, span,
                    );
                    return true;
                }
                ThreadControlKind::AsyncJoin => {
                    super::async_control::handle_async_join(
                        self, callee_func_name, args, target, *bb_idx, span,
                    );
                    return true;
                }
                ThreadControlKind::ScopeSpawn => {
                    self.handle_scope_spawn(callee_func_name, bb_idx, args, target);
                    return true;
                }
                ThreadControlKind::Join => {
                    self.handle_join(callee_func_name, args, target, *bb_idx, span);
                    return true;
                }
                ThreadControlKind::ScopeJoin => {
                    self.handle_scope_join(callee_func_name, args, target, *bb_idx, span);
                    return true;
                }
                ThreadControlKind::RayonJoin => {
                    self.handle_rayon_join(callee_func_name, bb_idx, args, target, span);
                    return true;
                }
            }
        }
        false
    }

    fn handle_scope_join(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        _target: &Option<BasicBlock>,
        bb_idx: BasicBlock,
        span: &str,
    ) {
        let join_id = AliasId::from_place(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().as_ref(),
        );
        let matching_callees = self.get_matching_spawn_callees(join_id);
        if matching_callees.is_empty() {
            log::error!(
                "No matching spawn call found for join in {:?}",
                self.instance.def_id()
            );
        }
        self.emit_tt(
            &TransitionType::Join(callee_func_name.to_string()),
            bb_idx,
            span,
        );
    }

    fn handle_scope_spawn(
        &mut self,
        _callee_func_name: &str,
        _bb_idx: &BasicBlock,
        _args: &Box<[Spanned<Operand<'tcx>>]>,
        _target: &Option<BasicBlock>,
    ) {
    }

    fn handle_rayon_join(
        &mut self,
        callee_func_name: &str,
        bb_idx: &BasicBlock,
        _args: &Box<[Spanned<Operand<'tcx>>]>,
        _target: &Option<BasicBlock>,
        span: &str,
    ) {
        self.emit_tt(
            &TransitionType::Join(callee_func_name.to_string()),
            *bb_idx,
            span,
        );
    }

    fn handle_spawn(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        destination: Local,
        _target: &Option<BasicBlock>,
        bb_idx: BasicBlock,
        span: &str,
    ) {
        if let Some((_closure_start, closure_end)) = self.resolve_closure_places(args) {
            self.ordered_spawn_ends.push_back(closure_end);
            self.spawn_handle_end.insert(destination, closure_end);
        }
        self.emit_tt(
            &TransitionType::Spawn(callee_func_name.to_string()),
            bb_idx,
            span,
        );
    }

    fn handle_join(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        _target: &Option<BasicBlock>,
        bb_idx: BasicBlock,
        span: &str,
    ) {
        let mut joined = false;
        if let Some(handle_local) = args.first().and_then(|a| a.node.place()).map(|p| p.local) {
            if let Some(vec_local) = self.handle_vec_source.get(&handle_local).copied() {
                let all_ends: Vec<_> = self
                    .vec_spawn_ends
                    .entry(vec_local)
                    .or_default()
                    .drain(..)
                    .collect();
                for _spawn_end in all_ends {
                    joined = true;
                }
            } else if let Some(vec_local) = self.option_vec_source.get(&handle_local).copied() {
                self.handle_vec_source.insert(handle_local, vec_local);
                let all_ends: Vec<_> = self
                    .vec_spawn_ends
                    .entry(vec_local)
                    .or_default()
                    .drain(..)
                    .collect();
                for _spawn_end in all_ends {
                    joined = true;
                }
            } else if self.spawn_handle_end.get(&handle_local).copied().is_some() {
                joined = true;
            }
        }

        if !joined && self.connect_join_from_recorded_order() {
            joined = true;
        }

        if !joined {
            let join_id = AliasId::from_place(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().as_ref(),
            );
            let matching_callees = self.get_matching_spawn_callees(join_id);
            if matching_callees.is_empty() {
                log::error!(
                    "No matching spawn call found for join in {:?}",
                    self.instance.def_id()
                );
            }
        }

        self.emit_tt(
            &TransitionType::Join(callee_func_name.to_string()),
            bb_idx,
            span,
        );
    }
}
