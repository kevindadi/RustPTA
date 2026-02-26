//! 调用处理：handle_call 主分发、handle_lock_call、handle_normal_call、handle_atomic_call、handle_condvar_call、handle_channel_call

use super::BodyToPetriNet;
use crate::{
    concurrency::blocking::{CondVarId, LockGuardId, LockGuardTy},
    memory::pointsto::AliasId,
    net::{Idx, PlaceId, TransitionId, TransitionType},
    util::has_pn_attribute,
};
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{BasicBlock, Operand};
use rustc_span::source_map::Spanned;

impl<'translate, 'analysis, 'tcx> BodyToPetriNet<'translate, 'analysis, 'tcx> {
    pub(super) fn handle_lock_call(
        &mut self,
        destination: &rustc_middle::mir::Place<'tcx>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) -> Option<TransitionType> {
        if cfg!(feature = "atomic-violation") {
            return None;
        }

        let lockguard_id = LockGuardId::new(self.instance_id, destination.local);
        if let Some(guard) = self.lockguards.get(&lockguard_id) {
            let lock_alias = lockguard_id.get_alias_id();
            let lock_node = self.resources.locks().get(&lock_alias).unwrap();

            let call_type = match &guard.lockguard_ty {
                LockGuardTy::StdMutex(_)
                | LockGuardTy::ParkingLotMutex(_)
                | LockGuardTy::SpinMutex(_) => TransitionType::Lock(lock_node.index()),
                LockGuardTy::StdRwLockRead(_)
                | LockGuardTy::ParkingLotRead(_)
                | LockGuardTy::SpinRead(_) => TransitionType::RwLockRead(lock_node.index()),
                _ => TransitionType::RwLockWrite(lock_node.index()),
            };

            self.update_lock_transition(bb_end, lock_node);
            self.connect_to_target(bb_end, target);
            Some(call_type)
        } else {
            None
        }
    }

    pub(super) fn update_lock_transition(&mut self, bb_end: TransitionId, lock_node: &PlaceId) {
        self.net.add_input_arc(*lock_node, bb_end, 1);
    }

    pub(super) fn handle_normal_call(
        &mut self,
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        name: &str,
        bb_idx: BasicBlock,
        span: &str,
        callee_id: &DefId,
        args: &Box<[Spanned<Operand<'tcx>>]>,
    ) {
        if let Some((callee_start, callee_end)) = self.functions_map().get(callee_id).copied() {
            let (_bb_wait, bb_ret) = crate::add_wait_ret_subnet!(
                self,
                name,
                bb_idx,
                "wait",
                "return",
                TransitionType::Function,
                span.to_string(),
                bb_end
            );

            self.net.add_output_arc(callee_start, bb_end, 1);
            if let Some(return_block) = target {
                self.net.add_input_arc(callee_end, bb_ret, 1);
                self.net
                    .add_output_arc(self.bb_graph.start(*return_block), bb_ret, 1);
            }
            return;
        }

        let name = self.tcx.def_path_str(callee_id);
        for i in 0..args.len() {
            if let Some((callee_start, callee_end)) = self.resolve_closure_places_at(args, i) {
                let (_bb_wait, bb_ret) = crate::add_wait_ret_subnet!(
                    self,
                    name,
                    bb_idx,
                    "wait",
                    "return",
                    TransitionType::Function,
                    span.to_string(),
                    bb_end
                );

                self.net.add_output_arc(callee_start, bb_end, 1);
                if let Some(return_block) = target {
                    self.net.add_input_arc(callee_end, bb_ret, 1);
                    self.net
                        .add_output_arc(self.bb_graph.start(*return_block), bb_ret, 1);
                }
                return;
            }
        }
        self.connect_to_target(bb_end, target);
    }

    pub(super) fn handle_atomic_call(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        if callee_func_name.contains("::load") {
            if !self.handle_atomic_load(args, bb_end, target, bb_idx, span) {
                log::debug!("no alias found for atomic load in {:?}", span);
                self.connect_to_target(bb_end, target);
            }
            return true;
        } else if callee_func_name.contains("::store") {
            if !self.handle_atomic_store(args, bb_end, target, bb_idx, span) {
                log::debug!("no alias found for atomic store in {:?}", span);
                self.connect_to_target(bb_end, target);
            }
            return true;
        } else if callee_func_name.contains("::compare_exchange") {
            false
        } else {
            false
        }
    }

    pub(super) fn handle_atomic_load(
        &mut self,
        args: &[Spanned<Operand<'tcx>>],
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        let current_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );

        let instance_index = self.instance_id.index();
        self.handle_atomic_basic_op(
            "load",
            current_id,
            bb_end,
            target,
            bb_idx,
            span,
            move |alias_id, order, span_str| {
                TransitionType::AtomicLoad(
                    alias_id.clone().into(),
                    order.clone(),
                    span_str,
                    instance_index,
                )
            },
        )
    }

    pub(super) fn handle_atomic_store(
        &mut self,
        args: &[Spanned<Operand<'tcx>>],
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        let current_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );
        let instance_index = self.instance_id.index();
        self.handle_atomic_basic_op(
            "store",
            current_id,
            bb_end,
            target,
            bb_idx,
            span,
            move |alias_id, order, span_str| {
                TransitionType::AtomicStore(
                    alias_id.clone().into(),
                    order.clone(),
                    span_str,
                    instance_index,
                )
            },
        )
    }

    pub(super) fn handle_condvar_call(
        &mut self,
        callee_def_id: DefId,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        name: &str,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        if cfg!(feature = "atomic-violation") {
            return false;
        }

        if has_pn_attribute(self.tcx, callee_def_id, "pn_condvar_notify")
            || self.key_api_regex.condvar_notify.is_match(callee_func_name)
        {
            let condvar_id = CondVarId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );
            let condvar_alias = condvar_id.get_alias_id();

            for (id, node) in self.resources.condvars().iter() {
                if self
                    .alias
                    .borrow_mut()
                    .alias_atomic(condvar_alias, *id)
                    .may_alias(self.alias_unknown_policy)
                {
                    self.net.add_output_arc(*node, bb_end, 1);

                    if let Some(transition) = self.net.get_transition_mut(bb_end) {
                        transition.transition_type = TransitionType::Notify(node.index());
                    }
                    break;
                }
            }
            self.connect_to_target(bb_end, target);
            true
        } else if has_pn_attribute(self.tcx, callee_def_id, "pn_condvar_wait")
            || self.key_api_regex.condvar_wait.is_match(callee_func_name)
        {
            let (_bb_wait, bb_ret) = crate::add_wait_ret_subnet!(
                self,
                name,
                bb_idx,
                "wait",
                "ret",
                TransitionType::Wait,
                span.to_string(),
                bb_end
            );

            let condvar_id = CondVarId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );
            let condvar_alias = condvar_id.get_alias_id();

            for (id, node) in self.resources.condvars().iter() {
                if self
                    .alias
                    .borrow_mut()
                    .alias_atomic(condvar_alias, *id)
                    .may_alias(self.alias_unknown_policy)
                {
                    self.net.add_input_arc(*node, bb_ret, 1);
                }
            }

            let guard_id = LockGuardId::new(
                self.instance_id,
                args.get(1).unwrap().node.place().unwrap().local,
            );
            let lock_alias = guard_id.get_alias_id();
            let lock_node = self.resources.locks().get(&lock_alias).unwrap();
            self.net.add_output_arc(*lock_node, bb_end, 1);
            self.net.add_input_arc(*lock_node, bb_ret, 1);

            self.connect_to_target(bb_ret, target);
            true
        } else {
            false
        }
    }

    pub(super) fn handle_channel_call(
        &mut self,
        callee_def_id: DefId,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
    ) -> bool {
        if cfg!(feature = "atomic-violation") {
            return false;
        }

        if self.resources.channel_places().is_empty() {
            return false;
        }

        if has_pn_attribute(self.tcx, callee_def_id, "pn_channel_send")
            || self.key_api_regex.channel_send.is_match(callee_func_name)
        {
            let channel_alias = AliasId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );

            if let Some(channel_node) = self.find_channel_place(channel_alias) {
                self.net.add_output_arc(channel_node, bb_end, 1);
                self.connect_to_target(bb_end, target);
                return true;
            }
        } else if has_pn_attribute(self.tcx, callee_def_id, "pn_channel_recv")
            || self.key_api_regex.channel_recv.is_match(callee_func_name)
        {
            let channel_alias = AliasId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );

            if let Some(channel_node) = self.find_channel_place(channel_alias) {
                self.net.add_input_arc(channel_node, bb_end, 1);
                self.connect_to_target(bb_end, target);
                return true;
            }
        }

        false
    }

    pub(super) fn handle_call(
        &mut self,
        bb_idx: BasicBlock,
        func: &Operand<'tcx>,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        destination: &rustc_middle::mir::Place<'tcx>,
        target: &Option<BasicBlock>,
        name: &str,
        span: &str,
        unwind: &rustc_middle::mir::UnwindAction,
    ) {
        match (target, unwind) {
            (None, rustc_middle::mir::UnwindAction::Continue) => {
                self.handle_unwind_continue(bb_idx, name);
                return;
            }
            (Some(t), _) => {
                if self.body.basic_blocks[*t].is_cleanup {
                    self.handle_panic(bb_idx, name);
                    return;
                }
            }
            _ => {}
        }

        let bb_term_name = crate::transition_name!(name, bb_idx, "call");
        let bb_end = self.create_call_transition(bb_idx, &bb_term_name);
        let callee_ty = func.ty(self.body, self.tcx);
        let callee_def_id = match callee_ty.kind() {
            rustc_middle::ty::TyKind::FnPtr(..) => {
                log::debug!("call fnptr: {:?}", callee_ty);
                self.connect_to_target(bb_end, target);
                return;
            }
            rustc_middle::ty::TyKind::FnDef(id, _) | rustc_middle::ty::TyKind::Closure(id, _) => {
                *id
            }
            _ => {
                panic!("TyKind::FnDef, a function definition, but got: {callee_ty:?}");
            }
        };

        let callee_func_name = crate::util::format_name(callee_def_id);

        if self.handle_lock_call(destination, target, bb_end).is_some() {
            log::debug!("callee_func_name with lock: {:?}", callee_func_name);
            return;
        }

        if self.handle_condvar_call(
            callee_def_id,
            &callee_func_name,
            args,
            bb_end,
            target,
            name,
            &bb_idx,
            span,
        ) {
            log::debug!("callee_func_name with condvar: {:?}", callee_func_name);
            return;
        }

        if callee_func_name.contains("::drop") && !cfg!(feature = "atomic-violation") {
            log::debug!("callee_func_name with drop: {:?}", callee_func_name);
            let lockguard_id = LockGuardId::new(
                self.instance_id,
                args.get(0).unwrap().node.place().unwrap().local,
            );
            if self.lockguards.get(&lockguard_id).is_some() {
                let lock_alias = lockguard_id.get_alias_id();
                let lock_node = self.resources.locks().get(&lock_alias).unwrap();
                match &self.lockguards[&lockguard_id].lockguard_ty {
                    LockGuardTy::StdMutex(_)
                    | LockGuardTy::ParkingLotMutex(_)
                    | LockGuardTy::SpinMutex(_) => {
                        self.net.add_output_arc(*lock_node, bb_end, 1);

                        if let Some(transition) = self.net.get_transition_mut(bb_end) {
                            transition.transition_type =
                                TransitionType::Unlock(lock_node.index());
                        }
                    }

                    LockGuardTy::StdRwLockRead(_)
                    | LockGuardTy::ParkingLotRead(_)
                    | LockGuardTy::SpinRead(_) => {
                        self.net.add_output_arc(*lock_node, bb_end, 1);

                        if let Some(transition) = self.net.get_transition_mut(bb_end) {
                            transition.transition_type =
                                TransitionType::Unlock(lock_node.index());
                        }
                    }
                    _ => {
                        self.net.add_output_arc(*lock_node, bb_end, 10);
                        if let Some(transition) = self.net.get_transition_mut(bb_end) {
                            transition.transition_type =
                                TransitionType::Unlock(lock_node.index());
                        }
                    }
                }
            }
            self.connect_to_target(bb_end, target);
            return;
        }

        if self.handle_channel_call(callee_def_id, &callee_func_name, args, bb_end, target) {
            log::debug!("callee_func_name with channel: {:?}", callee_func_name);
            return;
        }

        if self.handle_thread_call(
            callee_def_id,
            &callee_func_name,
            args,
            target,
            bb_end,
            &bb_idx,
            span,
        ) {
            log::debug!("callee_func_name with thread: {:?}", callee_func_name);
            return;
        }

        if self.handle_atomic_call(&callee_func_name, args, bb_end, target, &bb_idx, span) {
            log::debug!("callee_func_name with atomic: {:?}", callee_func_name);
            return;
        }

        log::debug!("callee_func_name with normal: {:?}", callee_func_name);
        if callee_func_name.contains("core::panic") {
            self.net.add_output_arc(self.entry_exit.1, bb_end, 1);
            return;
        }
        self.handle_normal_call(bb_end, target, name, bb_idx, span, &callee_def_id, args);
    }
}
