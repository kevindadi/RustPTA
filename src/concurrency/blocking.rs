extern crate rustc_hash;
extern crate rustc_span;

use std::collections::HashMap;

use rustc_middle::ty::{EarlyBinder, TyKind, TypingEnv};

use rustc_hash::FxHashMap;
use rustc_middle::mir::{Body, Local};
use rustc_middle::ty::{self, Instance, TyCtxt};
use rustc_span::Span;

use crate::graph::callgraph::InstanceId;

/// Uniquely identify a LockGuard in a crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LockGuardId {
    pub instance_id: InstanceId,
    pub local: Local,
}

impl LockGuardId {
    pub fn new(instance_id: InstanceId, local: Local) -> Self {
        Self { instance_id, local }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CondVarId {
    pub instance_id: InstanceId,
    pub local: Local,
}

impl CondVarId {
    pub fn new(instance_id: InstanceId, local: Local) -> Self {
        Self { instance_id, local }
    }
}

pub type CondvarMap<'tcx> = HashMap<CondVarId, String>;

/// LockGuardKind, DataTy
#[derive(Clone, Debug)]
pub enum LockGuardTy<'tcx> {
    StdMutex(ty::Ty<'tcx>),
    ParkingLotMutex(ty::Ty<'tcx>),
    SpinMutex(ty::Ty<'tcx>),
    StdRwLockRead(ty::Ty<'tcx>),
    StdRwLockWrite(ty::Ty<'tcx>),
    ParkingLotRead(ty::Ty<'tcx>),
    ParkingLotWrite(ty::Ty<'tcx>),
    SpinRead(ty::Ty<'tcx>),
    SpinWrite(ty::Ty<'tcx>),
}

impl<'tcx> LockGuardTy<'tcx> {
    pub fn from_local_ty(local_ty: ty::Ty<'tcx>, tcx: TyCtxt<'tcx>) -> Option<Self> {
        if let ty::TyKind::Adt(adt_def, substs) = local_ty.kind() {
            let path = tcx.def_path_str(adt_def.did());
            // quick fail
            if !path.contains("MutexGuard")
                && !path.contains("RwLockReadGuard")
                && !path.contains("RwLockWriteGuard")
            {
                return None;
            }
            let first_part = path.split('<').next()?;
            if first_part.contains("MutexGuard") {
                if first_part.contains("async")
                    || first_part.contains("tokio")
                    || first_part.contains("future")
                    || first_part.contains("loom")
                {
                    // Currentlly does not support async lock or loom
                    None
                } else if first_part.contains("spin") {
                    Some(LockGuardTy::SpinMutex(substs.types().next()?))
                } else if first_part.contains("lock_api") || first_part.contains("parking_lot") {
                    Some(LockGuardTy::ParkingLotMutex(substs.types().nth(1)?))
                } else {
                    // std::sync::Mutex or its wrapper by default
                    Some(LockGuardTy::StdMutex(substs.types().next()?))
                }
            } else if first_part.contains("RwLockReadGuard") {
                if first_part.contains("async")
                    || first_part.contains("tokio")
                    || first_part.contains("future")
                    || first_part.contains("loom")
                {
                    // Currentlly does not support async lock or loom
                    None
                } else if first_part.contains("spin") {
                    Some(LockGuardTy::SpinRead(substs.types().next()?))
                } else if first_part.contains("lock_api") || first_part.contains("parking_lot") {
                    Some(LockGuardTy::ParkingLotRead(substs.types().nth(1)?))
                } else {
                    // std::sync::RwLockReadGuard or its wrapper by default
                    Some(LockGuardTy::StdRwLockRead(substs.types().next()?))
                }
            } else if first_part.contains("RwLockWriteGuard") {
                if first_part.contains("async")
                    || first_part.contains("tokio")
                    || first_part.contains("future")
                    || first_part.contains("loom")
                {
                    // Currentlly does not support async lock or loom
                    None
                } else if first_part.contains("spin") {
                    Some(LockGuardTy::SpinWrite(substs.types().next()?))
                } else if first_part.contains("lock_api") || first_part.contains("parking_lot") {
                    Some(LockGuardTy::ParkingLotWrite(substs.types().nth(1)?))
                } else {
                    // std::sync::RwLockReadGuard or its wrapper by default
                    Some(LockGuardTy::StdRwLockWrite(substs.types().next()?))
                }
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// The lockguard info. `span` is for report.
#[derive(Clone, Debug)]
pub struct LockGuardInfo<'tcx> {
    pub lockguard_ty: LockGuardTy<'tcx>,
    pub span: Span,
}

impl<'tcx> LockGuardInfo<'tcx> {
    pub fn new(lockguard_ty: LockGuardTy<'tcx>, span: Span) -> Self {
        Self { lockguard_ty, span }
    }
}

pub type LockGuardMap<'tcx> = FxHashMap<LockGuardId, LockGuardInfo<'tcx>>;

/// Collect lockguard info.
pub struct BlockingCollector<'a, 'b, 'tcx> {
    instance_id: InstanceId,
    instance: &'a Instance<'tcx>,
    body: &'b Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    pub lockguards: LockGuardMap<'tcx>,
    pub condvars: CondvarMap<'tcx>,
}

impl<'a, 'b, 'tcx> BlockingCollector<'a, 'b, 'tcx> {
    pub fn new(
        instance_id: InstanceId,
        instance: &'a Instance<'tcx>,
        body: &'b Body<'tcx>,
        tcx: TyCtxt<'tcx>,
    ) -> Self {
        Self {
            instance_id,
            instance,
            body,
            tcx,
            lockguards: Default::default(),
            condvars: Default::default(),
        }
    }

    pub fn analyze(&mut self) {
        for (local, local_decl) in self.body.local_decls.iter_enumerated() {
            let typing_env = TypingEnv::post_analysis(self.tcx, self.instance.def_id());
            let local_ty = self.instance.instantiate_mir_and_normalize_erasing_regions(
                self.tcx,
                typing_env,
                EarlyBinder::bind(local_decl.ty),
            );
            if let Some(lockguard_ty) = LockGuardTy::from_local_ty(local_ty, self.tcx) {
                let lockguard_id = LockGuardId::new(self.instance_id, local);
                let lockguard_info = LockGuardInfo::new(lockguard_ty, local_decl.source_info.span);
                self.lockguards.insert(lockguard_id, lockguard_info);
            }

            if let TyKind::Adt(adt_def, _) = local_ty.kind() {
                let path = self.tcx.def_path_str(adt_def.did());
                if path.starts_with("std::sync::Condvar") {
                    self.condvars.insert(
                        CondVarId::new(self.instance_id, local),
                        format!("{:?}", local_decl.source_info.span),
                    );
                }
            }
        }
    }
}
