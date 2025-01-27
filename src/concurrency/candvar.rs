extern crate rustc_hash;
extern crate rustc_span;

use std::collections::HashMap;

use rustc_middle::mir::{Body, Local};
use rustc_middle::ty::TyKind;
use rustc_middle::ty::{self, Instance, TyCtxt, TypingEnv};

use crate::graph::callgraph::InstanceId;

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

/// Collect lockguard info.
pub struct CondVarCollector<'a, 'b, 'tcx> {
    instance_id: InstanceId,
    instance: &'a Instance<'tcx>,
    body: &'b Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    pub condvars: CondvarMap<'tcx>,
}

impl<'a, 'b, 'tcx> CondVarCollector<'a, 'b, 'tcx> {
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
            condvars: Default::default(),
        }
    }

    pub fn analyze(&mut self) {
        for (local, local_decl) in self.body.local_decls.iter_enumerated() {
            let typing_env = TypingEnv::post_analysis(self.tcx, self.instance.def_id());
            let local_ty = self.instance.instantiate_mir_and_normalize_erasing_regions(
                self.tcx,
                typing_env,
                ty::EarlyBinder::bind(local_decl.ty),
            );

            if let TyKind::Adt(adt_def, _) = local_ty.kind() {
                let path = self.tcx.def_path_str(adt_def.did());
                if path.contains("Condvar") {
                    self.condvars.insert(
                        CondVarId::new(self.instance_id, local),
                        format!("{:?}", local_decl.source_info.span),
                    );
                }
            }
        }
    }
}
