extern crate rustc_hash;
extern crate rustc_span;

use rustc_middle::ty::{EarlyBinder, TypingEnv};
use smallvec::SmallVec;
use std::cmp::Ordering;

use rustc_hash::FxHashMap;
use rustc_middle::mir::visit::{MutatingUseContext, NonMutatingUseContext, PlaceContext, Visitor};
use rustc_middle::mir::{Body, Local, Location, TerminatorKind};
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
    pub gen_locs: SmallVec<[Location; 4]>,
    pub move_gen_locs: SmallVec<[Location; 4]>,
    pub recursive_gen_locs: SmallVec<[Location; 4]>,
    pub kill_locs: SmallVec<[Location; 4]>,
}

impl<'tcx> LockGuardInfo<'tcx> {
    pub fn new(lockguard_ty: LockGuardTy<'tcx>, span: Span) -> Self {
        Self {
            lockguard_ty,
            span,
            gen_locs: Default::default(),
            move_gen_locs: Default::default(),
            recursive_gen_locs: Default::default(),
            kill_locs: Default::default(),
        }
    }

    pub fn is_gen_only_by_move(&self) -> bool {
        self.gen_locs == self.move_gen_locs
    }

    pub fn is_gen_only_by_recursive(&self) -> bool {
        self.gen_locs == self.recursive_gen_locs
    }
}

pub type LockGuardMap<'tcx> = FxHashMap<LockGuardId, LockGuardInfo<'tcx>>;

/// Collect lockguard info.
pub struct LockGuardCollector<'a, 'b, 'tcx> {
    instance_id: InstanceId,
    instance: &'a Instance<'tcx>,
    body: &'b Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    pub lockguards: LockGuardMap<'tcx>,
}

impl<'a, 'b, 'tcx> LockGuardCollector<'a, 'b, 'tcx> {
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
        }
    }

    pub fn analyze(&mut self) {
        for (local, local_decl) in self.body.local_decls.iter_enumerated() {
            // let local_ty = self.instance.instantiate_mir_and_normalize_erasing_regions(
            //     self.tcx,
            //     self.param_env,
            //     ty::EarlyBinder::bind(local_decl.ty),
            // );
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
        }
        self.visit_body(self.body);
    }
}

impl<'a, 'b, 'tcx> Visitor<'tcx> for LockGuardCollector<'a, 'b, 'tcx> {
    fn visit_local(&mut self, local: Local, context: PlaceContext, location: Location) {
        let lockguard_id = LockGuardId::new(self.instance_id, local);
        // local is lockguard
        if let Some(info) = self.lockguards.get_mut(&lockguard_id) {
            match context {
                PlaceContext::NonMutatingUse(NonMutatingUseContext::Move) => {
                    info.kill_locs.push(location);
                }
                PlaceContext::MutatingUse(context) => match context {
                    MutatingUseContext::Drop => info.kill_locs.push(location),
                    MutatingUseContext::Store => {
                        info.gen_locs.push(location);
                        info.move_gen_locs.push(location);
                    }
                    MutatingUseContext::Call => {
                        // if lockguard = parking_lot::recursive_read() then record to recursive_gen_locs
                        if let LockGuardTy::ParkingLotRead(_) = info.lockguard_ty {
                            let term = self.body[location.block].terminator();
                            if let TerminatorKind::Call { ref func, .. } = term.kind {
                                // Only after monomorphizing can Instance::resolve work
                                // let func_ty =
                                //     self.instance.instantiate_mir_and_normalize_erasing_regions(
                                //         self.tcx,
                                //         self.param_env,
                                //         ty::EarlyBinder::bind(func.ty(self.body, self.tcx)),
                                //     );
                                let typing_env =
                                    TypingEnv::post_analysis(self.tcx, self.instance.def_id());
                                let func_ty = ty::EarlyBinder::bind(func.ty(self.body, self.tcx));
                                let func_ty =
                                    self.instance.instantiate_mir_and_normalize_erasing_regions(
                                        self.tcx, typing_env, func_ty,
                                    );
                                if let ty::FnDef(def_id, _) = *func_ty.kind() {
                                    let fn_name = self.tcx.def_path_str(def_id);
                                    if fn_name.contains("read_recursive") {
                                        info.recursive_gen_locs.push(location);
                                    }
                                }
                            }
                        }
                        info.gen_locs.push(location);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
}
