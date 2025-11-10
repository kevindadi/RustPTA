use petgraph::algo;
use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
use petgraph::visit::IntoNodeReferences;
use petgraph::Direction::Incoming;
use petgraph::{Directed, Graph};

use std::fs;
use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};
use rustc_hir::def_id::DefId;
use rustc_middle::mir::visit::Visitor;
use rustc_middle::mir::{
    Body, Local, LocalDecl, LocalKind, Location, Operand, Terminator, TerminatorKind,
};
use rustc_middle::ty::{self, Instance, TyCtxt, TyKind, TypingEnv};

pub type InstanceId = NodeIndex;

#[derive(Copy, Clone, Debug)]
pub enum CallSiteLocation {
    Direct(Location),
    ClosureDef(Local),

    Spawn {
        location: Location,
        destination: Local,
    },
    RayonJoin,
}

impl CallSiteLocation {
    pub fn location(&self) -> Option<Location> {
        match self {
            Self::Direct(loc) => Some(*loc),
            Self::Spawn { location, .. } => Some(*location),
            _ => None,
        }
    }

    pub fn spawn_destination(&self) -> Option<Local> {
        match self {
            Self::Spawn { destination, .. } => Some(*destination),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct FunctionNode<'tcx> {
    pub instance: Instance<'tcx>,
    pub def_id: DefId,
    pub name: Box<str>,
}

impl<'tcx> FunctionNode<'tcx> {
    pub fn new_node(instance: Instance<'tcx>, def_id: DefId) -> FunctionNode<'tcx> {
        FunctionNode {
            instance,
            def_id,
            name: FunctionNode::format_name(def_id),
        }
    }

    pub fn format_name(def_id: DefId) -> Box<str> {
        let tmp1 = format!("{def_id:?}");
        let tmp2: &str = tmp1.split("~ ").collect::<Vec<&str>>()[1];
        let tmp3 = tmp2.replace(')', "");
        let lhs = tmp3.split('[').collect::<Vec<&str>>()[0];
        let rhs = tmp3.split(']').collect::<Vec<&str>>()[1];
        format!("{lhs}{rhs}").into_boxed_str()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CallGraphNode<'tcx> {
    WithBody(Instance<'tcx>),
    WithoutBody(Instance<'tcx>),
}

impl<'tcx> CallGraphNode<'tcx> {
    pub fn instance(&self) -> &Instance<'tcx> {
        match self {
            CallGraphNode::WithBody(inst) | CallGraphNode::WithoutBody(inst) => inst,
        }
    }

    pub fn match_instance(&self, other: &Instance<'tcx>) -> bool {
        matches!(self, CallGraphNode::WithBody(inst) | CallGraphNode::WithoutBody(inst) if inst == other)
    }
}

pub struct CallGraph<'tcx> {
    pub graph: Graph<CallGraphNode<'tcx>, Vec<CallSiteLocation>, Directed>,

    pub spawn_calls: FxHashMap<DefId, FxHashSet<(DefId, Local)>>,
}

impl<'tcx> CallGraph<'tcx> {
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
            spawn_calls: FxHashMap::default(),
        }
    }

    pub fn format_spawn_calls(&self) -> String {
        let mut output = String::from("Spawn calls in functions:\n");

        for (caller_id, spawn_set) in &self.spawn_calls {
            let caller_name = FunctionNode::format_name(*caller_id);
            output.push_str(&format!("\nIn function {}:\n", caller_name));

            for (closure_id, destination) in spawn_set {
                let closure_name = FunctionNode::format_name(*closure_id);
                output.push_str(&format!(
                    "  - Spawned closure {} (stored in _{})\n",
                    closure_name,
                    destination.index()
                ));
            }
        }
        output
    }

    pub fn instance_to_index(&self, instance: &Instance<'tcx>) -> Option<InstanceId> {
        self.graph
            .node_references()
            .find(|(_idx, inst)| inst.match_instance(instance))
            .map(|(idx, _)| idx)
    }

    pub fn index_to_instance(&self, idx: InstanceId) -> Option<&CallGraphNode<'tcx>> {
        self.graph.node_weight(idx)
    }

    fn record_spawn_call(&mut self, caller: DefId, closure_idx: DefId, destination: Local) {
        self.spawn_calls
            .entry(caller)
            .or_default()
            .insert((closure_idx, destination));
    }

    pub fn get_spawn_calls(&self, def_id: DefId) -> Option<&FxHashSet<(DefId, Local)>> {
        self.spawn_calls.get(&def_id)
    }

    pub fn analyze(&mut self, instances: Vec<Instance<'tcx>>, tcx: TyCtxt<'tcx>) {
        let idx_insts = instances
            .into_iter()
            .map(|inst| {
                let idx = self.graph.add_node(CallGraphNode::WithBody(inst));
                (idx, inst)
            })
            .collect::<Vec<_>>();
        for (caller_idx, caller) in idx_insts {
            let body = tcx.instance_mir(caller.def);

            if body.source.promoted.is_some() {
                continue;
            }
            let mut collector = CallSiteCollector::new(caller, body, tcx);
            collector.visit_body(body);
            for (callee, location) in collector.finish() {
                let callee_idx = if let Some(callee_idx) = self.instance_to_index(&callee) {
                    callee_idx
                } else {
                    self.graph.add_node(CallGraphNode::WithoutBody(callee))
                };

                if let CallSiteLocation::Spawn { destination, .. } = location {
                    self.record_spawn_call(caller.def_id(), callee.def_id(), destination);
                }
                if let Some(edge_idx) = self.graph.find_edge(caller_idx, callee_idx) {
                    self.graph.edge_weight_mut(edge_idx).unwrap().push(location);
                } else {
                    self.graph.add_edge(caller_idx, callee_idx, vec![location]);
                }
            }
        }
    }

    pub fn callsites(
        &self,
        source: InstanceId,
        target: InstanceId,
    ) -> Option<Vec<CallSiteLocation>> {
        let edge = self.graph.find_edge(source, target)?;
        self.graph.edge_weight(edge).cloned()
    }

    pub fn callers(&self, target: InstanceId) -> Vec<InstanceId> {
        self.graph.neighbors_directed(target, Incoming).collect()
    }

    pub fn all_simple_paths(&self, source: InstanceId, target: InstanceId) -> Vec<Vec<InstanceId>> {
        algo::all_simple_paths::<Vec<_>, _>(&self.graph, source, target, 0, None)
            .collect::<Vec<_>>()
    }

    #[allow(dead_code)]
    pub fn dot(&self) -> String {
        format!(
            "{:?}",
            Dot::with_config(&self.graph, &[Config::EdgeNoLabel])
        )
    }

    pub fn write_dot<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let dot = self.dot();
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, dot)
    }
}

struct CallSiteCollector<'a, 'tcx> {
    caller: Instance<'tcx>,
    body: &'a Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    callsites: Vec<(Instance<'tcx>, CallSiteLocation)>,
}

impl<'a, 'tcx> CallSiteCollector<'a, 'tcx> {
    fn new(caller: Instance<'tcx>, body: &'a Body<'tcx>, tcx: TyCtxt<'tcx>) -> Self {
        Self {
            caller,
            body,
            tcx,
            callsites: Vec::new(),
        }
    }

    fn finish(self) -> impl IntoIterator<Item = (Instance<'tcx>, CallSiteLocation)> {
        self.callsites.into_iter()
    }
}

impl<'a, 'tcx> Visitor<'tcx> for CallSiteCollector<'a, 'tcx> {
    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        if let TerminatorKind::Call {
            ref func,
            ref args,
            destination,
            ..
        } = terminator.kind
        {
            let typing_env = TypingEnv::post_analysis(self.tcx, self.caller.def_id());
            let func_ty = self.caller.instantiate_mir_and_normalize_erasing_regions(
                self.tcx,
                typing_env,
                ty::EarlyBinder::bind(func.ty(self.body, self.tcx)),
            );

            if let ty::FnDef(def_id, substs) = *func_ty.kind() {
                let fn_path = self.tcx.def_path_str(def_id);
                if fn_path.contains("spawn") {
                    if let Some(closure_arg) = args.first() {
                        let closure_ty = match closure_arg.node {
                            Operand::Move(place) | Operand::Copy(place) => {
                                place.ty(self.body, self.tcx).ty
                            }
                            Operand::Constant(ref const_op) => const_op.ty(),
                        };

                        if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                            closure_ty.kind()
                        {
                            if let Some(callee) =
                                Instance::try_resolve(self.tcx, typing_env, *closure_def_id, substs)
                                    .ok()
                                    .flatten()
                            {
                                self.callsites.push((
                                    callee,
                                    CallSiteLocation::Spawn {
                                        location,
                                        destination: destination.local,
                                    },
                                ));

                                return;
                            }
                        }
                    }

                    if let Some(closure_arg) = args.get(1) {
                        let closure_ty = match closure_arg.node {
                            Operand::Move(place) | Operand::Copy(place) => {
                                place.ty(self.body, self.tcx).ty
                            }
                            Operand::Constant(ref const_op) => const_op.ty(),
                        };

                        if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                            closure_ty.kind()
                        {
                            if let Some(callee) =
                                Instance::try_resolve(self.tcx, typing_env, *closure_def_id, substs)
                                    .ok()
                                    .flatten()
                            {
                                self.callsites.push((
                                    callee,
                                    CallSiteLocation::Spawn {
                                        location,
                                        destination: destination.local,
                                    },
                                ));

                                return;
                            }
                        }
                    }
                }

                if fn_path.contains("rayon_core::join") {
                    for arg in args {
                        if let Operand::Move(place) | Operand::Copy(place) = arg.node {
                            let place_ty = place.ty(self.body, self.tcx).ty;
                            if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                                place_ty.kind()
                            {
                                if let Some(callee) = Instance::try_resolve(
                                    self.tcx,
                                    typing_env,
                                    *closure_def_id,
                                    substs,
                                )
                                .ok()
                                .flatten()
                                {
                                    self.callsites.push((callee, CallSiteLocation::RayonJoin));
                                }
                            }
                        }
                    }
                    return;
                }

                if let Some(callee) = Instance::try_resolve(self.tcx, typing_env, def_id, substs)
                    .ok()
                    .flatten()
                {
                    self.callsites
                        .push((callee, CallSiteLocation::Direct(location)));
                }
            }
        }
        self.super_terminator(terminator, location);
    }

    fn visit_local_decl(&mut self, local: Local, local_decl: &LocalDecl<'tcx>) {
        let typing_env = TypingEnv::post_analysis(self.tcx, self.caller.def_id());
        let func_ty = self.caller.instantiate_mir_and_normalize_erasing_regions(
            self.tcx,
            typing_env,
            ty::EarlyBinder::bind(local_decl.ty),
        );
        if let TyKind::Closure(def_id, substs) = *func_ty.kind() {
            match self.body.local_kind(local) {
                LocalKind::Arg | LocalKind::ReturnPointer => {}
                _ => {
                    if let Some(callee_instance) =
                        Instance::try_resolve(self.tcx, typing_env, def_id, substs)
                            .ok()
                            .flatten()
                    {
                        self.callsites
                            .push((callee_instance, CallSiteLocation::ClosureDef(local)));
                    }
                }
            }
        }
        self.super_local_decl(local, local_decl);
    }
}
