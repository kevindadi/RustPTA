//! 调用处理（与 `mir_to_pn/calls` 平行，不建网，仅发射 CIR）

use super::BodyToCir;
use crate::{
    concurrency::blocking::{CondVarId, LockGuardId, LockGuardTy},
    memory::pointsto::AliasId,
    net::Idx,
    net::structure::TransitionType,
    util::has_pn_attribute,
};
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{BasicBlock, Operand};
use rustc_span::source_map::Spanned;

impl<'translate, 'analysis, 'tcx, 'a> BodyToCir<'translate, 'analysis, 'tcx, 'a> {
    fn lock_transition_type(
        &self,
        destination: &rustc_middle::mir::Place<'tcx>,
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
            Some(call_type)
        } else {
            None
        }
    }

    pub(super) fn handle_atomic_call(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        if callee_func_name.contains("::load") {
            if !self.handle_atomic_load(args, bb_idx, span) {
                log::debug!("no alias found for atomic load in {:?}", span);
            }
            return true;
        } else if callee_func_name.contains("::store") {
            if !self.handle_atomic_store(args, bb_idx, span) {
                log::debug!("no alias found for atomic store in {:?}", span);
            }
            return true;
        } else if callee_func_name.contains("::compare_exchange") {
            false
        } else {
            false
        }
    }

    fn handle_atomic_load(
        &mut self,
        args: &[Spanned<Operand<'tcx>>],
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

    fn handle_atomic_store(
        &mut self,
        args: &[Spanned<Operand<'tcx>>],
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

    fn handle_condvar_call(
        &mut self,
        callee_def_id: DefId,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
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
                    self.emit_tt(&TransitionType::Notify(node.index()), *bb_idx, span);
                    break;
                }
            }
            true
        } else if has_pn_attribute(self.tcx, callee_def_id, "pn_condvar_wait")
            || self.key_api_regex.condvar_wait.is_match(callee_func_name)
        {
            self.emit_tt(&TransitionType::Wait, *bb_idx, span);
            true
        } else {
            false
        }
    }

    fn handle_channel_call(
        &mut self,
        callee_def_id: DefId,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        _bb_idx: BasicBlock,
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
            if self.find_channel_place(channel_alias).is_some() {
                return true;
            }
        } else if has_pn_attribute(self.tcx, callee_def_id, "pn_channel_recv")
            || self.key_api_regex.channel_recv.is_match(callee_func_name)
        {
            let channel_alias = AliasId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );
            if self.find_channel_place(channel_alias).is_some() {
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
        _name: &str,
        span: &str,
        unwind: &rustc_middle::mir::UnwindAction,
    ) {
        match (target, unwind) {
            (None, rustc_middle::mir::UnwindAction::Continue) => {
                return;
            }
            (Some(t), _) => {
                if self.body.basic_blocks[*t].is_cleanup {
                    return;
                }
            }
            _ => {}
        }

        let callee_ty = func.ty(self.body, self.tcx);
        let callee_def_id = match callee_ty.kind() {
            rustc_middle::ty::TyKind::FnPtr(..) => {
                log::debug!("call fnptr: {:?}", callee_ty);
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
        log::debug!("[mir_to_cir] track_joinhandle: {}", callee_func_name);
        self.track_joinhandle_container_call(&callee_func_name, args, destination.local);

        if let Some(tt) = self.lock_transition_type(destination) {
            log::debug!("callee_func_name with lock: {:?}", callee_func_name);
            self.emit_tt(&tt, bb_idx, span);
            return;
        }

        if self.handle_condvar_call(callee_def_id, &callee_func_name, args, &bb_idx, span) {
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
                self.emit_tt(
                    &TransitionType::Unlock(lock_node.index()),
                    bb_idx,
                    span,
                );
            }
            return;
        }

        if self.handle_channel_call(callee_def_id, &callee_func_name, args, bb_idx) {
            log::debug!("callee_func_name with channel: {:?}", callee_func_name);
            return;
        }

        if self.handle_thread_call(
            callee_def_id,
            &callee_func_name,
            args,
            destination.local,
            target,
            &bb_idx,
            span,
        ) {
            log::debug!("callee_func_name with thread: {:?}", callee_func_name);
            return;
        }

        if self.handle_atomic_call(&callee_func_name, args, &bb_idx, span) {
            log::debug!("callee_func_name with atomic: {:?}", callee_func_name);
            return;
        }

        log::debug!("callee_func_name with normal: {:?}", callee_func_name);
        if callee_func_name.contains("core::panic") {
            return;
        }
        self.emit_call_if_in_scope(callee_def_id, bb_idx);
    }
}
