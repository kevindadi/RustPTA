//! Denotes Condvar APIs in std and parking_lot.
//!
//! 1. std::Condvar::wait.*(&Condvar, MutexGuard,.*) -> MutexGuard
//! 2. std::Condvar::notify.*(&Condvar)
//! 3. parking_lot::Condvar::wait.*(&Condvar, &mut MutexGuard,.*)
//! 4. parking_lot::Condvar::notify.*(&Condvar)
extern crate rustc_hash;
extern crate rustc_span;

use smallvec::SmallVec;

use rustc_hash::FxHashMap;
use rustc_middle::mir::visit::{MutatingUseContext, NonMutatingUseContext, PlaceContext, Visitor};
use rustc_middle::mir::{Body, Local, Location, TerminatorKind};
use rustc_middle::ty::{self, Instance, ParamEnv, TyCtxt};
use rustc_span::Span;

use crate::graph::callgraph::InstanceId;

#[derive(Clone, Copy, Debug)]
pub enum CondvarApi {
    Std(StdCondvarApi),
    ParkingLot(ParkingLotCondvarApi),
}

impl CondvarApi {
    pub fn from_instance<'tcx>(instance: &Instance<'tcx>, tcx: TyCtxt<'tcx>) -> Option<Self> {
        let path = tcx.def_path_str_with_args(instance.def_id(), instance.args); //
        let std_condvar = "std::sync::Condvar::";
        let parking_lot_condvar = "parking_lot::Condvar::";
        if path.starts_with(std_condvar) {
            let tail = &path.as_bytes()[std_condvar.len()..];
            let std_condvar_api = if tail.starts_with("wait::".as_bytes()) {
                StdCondvarApi::Wait(StdWait::Wait)
            } else if tail.starts_with("wait_timeout::".as_bytes()) {
                StdCondvarApi::Wait(StdWait::WaitTimeout)
            } else if tail.starts_with("wait_timeout_ms::".as_bytes()) {
                StdCondvarApi::Wait(StdWait::WaitTimeoutMs)
            } else if tail.starts_with("wait_timeout_while::".as_bytes()) {
                StdCondvarApi::Wait(StdWait::WaitTimeoutWhile)
            } else if tail.starts_with("wait_while::".as_bytes()) {
                StdCondvarApi::Wait(StdWait::WaitWhile)
            } else if tail == "notify_all".as_bytes() {
                StdCondvarApi::Notify(StdNotify::NotifyAll)
            } else if tail == "notify_one".as_bytes() {
                StdCondvarApi::Notify(StdNotify::NotifyOne)
            } else {
                return None;
            };
            Some(CondvarApi::Std(std_condvar_api))
        } else if path.starts_with(parking_lot_condvar) {
            let tail = &path.as_bytes()[parking_lot_condvar.len()..];
            let parking_lot_condvar_api = if tail.starts_with("wait::".as_bytes()) {
                ParkingLotCondvarApi::Wait(ParkingLotWait::Wait)
            } else if tail.starts_with("wait_for::".as_bytes()) {
                ParkingLotCondvarApi::Wait(ParkingLotWait::WaitFor)
            } else if tail.starts_with("wait_until::".as_bytes()) {
                ParkingLotCondvarApi::Wait(ParkingLotWait::WaitUntil)
            } else if tail.starts_with("wait_while::".as_bytes()) {
                ParkingLotCondvarApi::Wait(ParkingLotWait::WaitWhile)
            } else if tail.starts_with("wait_while_for::".as_bytes()) {
                ParkingLotCondvarApi::Wait(ParkingLotWait::WaitWhileFor)
            } else if tail.starts_with("wait_while_until::".as_bytes()) {
                ParkingLotCondvarApi::Wait(ParkingLotWait::WaitWhileUntil)
            } else if tail == "notify_all".as_bytes() {
                ParkingLotCondvarApi::Notify(ParkingLotNotify::NotifyAll)
            } else if tail == "notify_one".as_bytes() {
                ParkingLotCondvarApi::Notify(ParkingLotNotify::NotifyOne)
            } else {
                return None;
            };
            Some(CondvarApi::ParkingLot(parking_lot_condvar_api))
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum StdCondvarApi {
    Wait(StdWait),
    Notify(StdNotify),
}

#[derive(Clone, Copy, Debug)]
pub enum StdWait {
    Wait,
    WaitTimeout,
    WaitTimeoutMs,
    WaitTimeoutWhile,
    WaitWhile,
}

#[derive(Clone, Copy, Debug)]
pub enum StdNotify {
    NotifyAll,
    NotifyOne,
}

#[derive(Clone, Copy, Debug)]
pub enum ParkingLotCondvarApi {
    Wait(ParkingLotWait),
    Notify(ParkingLotNotify),
}

#[derive(Clone, Copy, Debug)]
pub enum ParkingLotWait {
    Wait,
    WaitFor,
    WaitUntil,
    WaitWhile,
    WaitWhileFor,
    WaitWhileUntil,
}

#[derive(Clone, Copy, Debug)]
pub enum ParkingLotNotify {
    NotifyAll,
    NotifyOne,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct CondVarId {
    pub instance_id: InstanceId,
    pub local: Local,
}

impl CondVarId {
    pub fn new(instance_id: InstanceId, local: Local) -> Self {
        Self { instance_id, local }
    }
}

#[derive(Debug, Clone)]
pub enum CandVarTy<'tcx> {
    StdCandvar(ty::Ty<'tcx>),
}

impl<'tcx> CandVarTy<'tcx> {
    pub fn from_local_ty(local_ty: ty::Ty<'tcx>, tcx: TyCtxt<'tcx>) -> Option<Self> {
        if let ty::TyKind::Adt(adt_def, substs) = local_ty.kind() {
            let path = tcx.def_path_str(adt_def.did());
            // quick fail
            if path.contains("Candvar") {
                if path.contains("async")
                    || path.contains("tokio")
                    || path.contains("future")
                    || path.contains("loom")
                {
                    // Currentlly does not support async lock or loom
                    return None;
                }
                Some(CandVarTy::StdCandvar(substs.types().next()?))
            } else {
                None
            }
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub struct CandVarInfo<'tcx> {
    pub candvar_ty: CandVarTy<'tcx>,
    pub span: Span,
    pub gen_locs: SmallVec<[Location; 4]>,
    pub move_gen_locs: SmallVec<[Location; 4]>,
    pub recursive_gen_locs: SmallVec<[Location; 4]>,
    pub kill_locs: SmallVec<[Location; 4]>,
}

impl<'tcx> CandVarInfo<'tcx> {
    pub fn new(candvar_ty: CandVarTy<'tcx>, span: Span) -> Self {
        Self {
            candvar_ty,
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

pub type CandvarMap<'tcx> = FxHashMap<CondVarId, CandVarInfo<'tcx>>;

/// Collect lockguard info.
pub struct CandVarCollector<'a, 'b, 'tcx> {
    instance_id: InstanceId,
    instance: &'a Instance<'tcx>,
    body: &'b Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    param_env: ParamEnv<'tcx>,
    candvars: CandvarMap<'tcx>,
}

impl<'a, 'b, 'tcx> CandVarCollector<'a, 'b, 'tcx> {
    pub fn new(
        instance_id: InstanceId,
        instance: &'a Instance<'tcx>,
        body: &'b Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        param_env: ParamEnv<'tcx>,
    ) -> Self {
        Self {
            instance_id,
            instance,
            body,
            tcx,
            param_env,
            candvars: Default::default(),
        }
    }

    pub fn analyze(&mut self) {
        for (local, local_decl) in self.body.local_decls.iter_enumerated() {
            // let local_ty = self.instance.instantiate_mir_and_normalize_erasing_regions(
            //     self.tcx,
            //     self.param_env,
            //     ty::EarlyBinder::bind(local_decl.ty),
            // );
            let local_ty = self.instance.subst_mir_and_normalize_erasing_regions(
                self.tcx,
                self.param_env,
                ty::EarlyBinder::bind(local_decl.ty),
            );
            if let Some(candvar_ty) = CandVarTy::from_local_ty(local_ty, self.tcx) {
                let candvar_id = CondVarId::new(self.instance_id, local);
                let candvar_info = CandVarInfo::new(candvar_ty, local_decl.source_info.span);
                self.candvars.insert(candvar_id, candvar_info);
            }
        }
        self.visit_body(self.body);
    }
}

impl<'a, 'b, 'tcx> Visitor<'tcx> for CandVarCollector<'a, 'b, 'tcx> {
    fn visit_local(&mut self, local: Local, context: PlaceContext, location: Location) {
        let lockguard_id = CondVarId::new(self.instance_id, local);
        // local is lockguard
        if let Some(info) = self.candvars.get_mut(&lockguard_id) {
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
                        info.gen_locs.push(location);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
}
