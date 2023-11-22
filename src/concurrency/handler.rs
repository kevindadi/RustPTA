//! Collect LockGuard info.
extern crate rustc_hash;
extern crate rustc_span;

use rustc_span::def_id::DefId;
use smallvec::SmallVec;

use rustc_hash::FxHashMap;
use rustc_middle::mir::visit::{MutatingUseContext, NonMutatingUseContext, PlaceContext, Visitor};
use rustc_middle::mir::{Body, Local, Location};
use rustc_middle::ty::{self, Instance, ParamEnv, TyCtxt};
use rustc_span::Span;

use crate::graph::callgraph::InstanceId;

/// Uniquely identify a LockGuard in a crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JoinHanderId {
    pub instance_id: InstanceId,
    pub local: Local,
}

impl JoinHanderId {
    pub fn new(instance_id: InstanceId, local: Local) -> Self {
        Self { instance_id, local }
    }
}

/// LockGuardKind, DataTy
#[derive(Clone, Debug)]
pub enum HandlerTy<'tcx> {
    StdJoinHandler(ty::Ty<'tcx>),
    TokioJoinHandler(ty::Ty<'tcx>),
}

impl<'tcx> HandlerTy<'tcx> {
    pub fn from_local_ty(local_ty: ty::Ty<'tcx>, tcx: TyCtxt<'tcx>) -> Option<Self> {
        if let ty::TyKind::Adt(adt_def, substs) = local_ty.kind() {
            let path = tcx.def_path_str(adt_def.did());
            // quick fail
            if !path.contains("Join") {
                return None;
            }
            let first_part = path.split('<').next()?;
            if first_part.contains("JoinHandle") {
                if first_part.contains("async")
                    || first_part.contains("tokio")
                    || first_part.contains("future")
                    || first_part.contains("loom")
                {
                    // Currentlly does not support async lock or loom
                    None
                } else {
                    // std::sync::Mutex or its wrapper by default
                    Some(HandlerTy::StdJoinHandler(substs.types().next()?))
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
pub struct JoinHandlerInfo<'tcx> {
    pub handler_info: HandlerTy<'tcx>,
    pub span: Span,
    pub gen_locs: SmallVec<[Location; 4]>,
    pub move_gen_locs: SmallVec<[Location; 4]>,
    pub recursive_gen_locs: SmallVec<[Location; 4]>,
    pub kill_locs: SmallVec<[Location; 4]>,
    // pub thread_id: DefId,
}

impl<'tcx> JoinHandlerInfo<'tcx> {
    pub fn new(handler_info: HandlerTy<'tcx>, span: Span) -> Self {
        Self {
            handler_info,
            span,
            gen_locs: Default::default(),
            move_gen_locs: Default::default(),
            recursive_gen_locs: Default::default(),
            kill_locs: Default::default(),
            // thread_id,
        }
    }

    pub fn is_gen_only_by_move(&self) -> bool {
        self.gen_locs == self.move_gen_locs
    }

    pub fn is_gen_only_by_recursive(&self) -> bool {
        self.gen_locs == self.recursive_gen_locs
    }
}

pub type JoinHandlerMap<'tcx> = FxHashMap<JoinHanderId, JoinHandlerInfo<'tcx>>;

/// Collect joinhandler info.
pub struct JoinHandlerCollector<'a, 'b, 'tcx> {
    instance_id: InstanceId,
    instance: &'a Instance<'tcx>,
    body: &'b Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    param_env: ParamEnv<'tcx>,
    pub handlers: JoinHandlerMap<'tcx>,
}

impl<'a, 'b, 'tcx> JoinHandlerCollector<'a, 'b, 'tcx> {
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
            handlers: Default::default(),
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
            if let Some(handler_ty) = HandlerTy::from_local_ty(local_ty, self.tcx) {
                let handler_id = JoinHanderId::new(self.instance_id, local);
                let handler_id_info = JoinHandlerInfo::new(handler_ty, local_decl.source_info.span);
                self.handlers.insert(handler_id, handler_id_info);
            }
        }
        self.visit_body(self.body);
    }
}

impl<'a, 'b, 'tcx> Visitor<'tcx> for JoinHandlerCollector<'a, 'b, 'tcx> {
    fn visit_local(&mut self, local: Local, context: PlaceContext, location: Location) {
        let handler_id = JoinHanderId::new(self.instance_id, local);
        // local is lockguard
        if let Some(info) = self.handlers.get_mut(&handler_id) {
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
                    _ => {}
                },
                _ => {}
            }
        }
    }
}
