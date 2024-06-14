//! Generate a CallGraph for instances in each crate.
//! You can roughly think of instances as a monomorphic function.
//! If an instance calls another instance, then we have an edge
//! from caller to callee with callsite locations as edge weight.
//! This is a fundamental analysis for other analysis,
//! e.g., points-to analysis, lockguard collector, etc.
//! We also track where a closure is defined rather than called
//! to record the defined function and the parameter of the closure,
//! which is pointed to by upvars.
use std::fmt::Debug;

use petgraph::algo;
use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
use petgraph::visit::IntoNodeReferences;
use petgraph::Direction::Incoming;
use petgraph::{Directed, Graph};

use rustc_hir::def_id::DefId;
use rustc_middle::mir::visit::Visitor;
use rustc_middle::mir::{Body, Local, LocalDecl, LocalKind, Location, Terminator, TerminatorKind};
use rustc_middle::ty::{self, Instance, ParamEnv, TyCtxt, TyKind};

use crate::concurrency::locks::LockGuardId;

/// The NodeIndex in CallGraph, denoting a unique instance in CallGraph.
pub type InstanceId = NodeIndex;

/// The location where caller calls callee.
/// Support direct call for now, where callee resolves to FnDef.
/// Also support tracking the parameter of a closure (pointed to by upvars)
/// Add support for FnPtr.
#[derive(Copy, Clone, Debug)]
pub enum CallSiteLocation {
    Direct(Location),
    ClosureDef(Local),
    // Indirect(Location),
}

impl CallSiteLocation {
    pub fn location(&self) -> Option<Location> {
        match self {
            Self::Direct(loc) => Some(*loc),
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
    pub fn new_node(instance: Instance<'tcx>, def_id: DefId) -> FunctionNode {
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

/// The CallGraph node wrapping an Instance.
/// WithBody means the Instance owns body.
#[derive(Debug, PartialEq, Eq)]
pub enum CallGraphNode<'tcx> {
    WithBody(Instance<'tcx>),
    WithoutBody(Instance<'tcx>),
}

// #[derive(Debug, PartialEq, Eq)]
// pub enum CallGraphNode<'tcx> {
//     WithBody(FunctionNode<'tcx>),
//     WithoutBody(FunctionNode<'tcx>),
// }

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

/// CallGraph
/// The nodes of CallGraph are instances.
/// The directed edges are CallSite Locations.
/// e.g., `Instance1--|[CallSite1, CallSite2]|-->Instance2`
/// denotes `Instance1` calls `Instance2` at locations `Callsite1` and `CallSite2`.
pub struct CallGraph<'tcx> {
    pub graph: Graph<CallGraphNode<'tcx>, Vec<CallSiteLocation>, Directed>,
}

impl<'tcx> CallGraph<'tcx> {
    /// Create an empty CallGraph.
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
        }
    }

    /// Search for the InstanceId of a given instance in CallGraph.
    pub fn instance_to_index(&self, instance: &Instance<'tcx>) -> Option<InstanceId> {
        self.graph
            .node_references()
            .find(|(_idx, inst)| inst.match_instance(instance))
            .map(|(idx, _)| idx)
    }

    /// Get the instance by InstanceId.
    pub fn index_to_instance(&self, idx: InstanceId) -> Option<&CallGraphNode<'tcx>> {
        self.graph.node_weight(idx)
    }

    /// Perform callgraph analysis on the given instances.
    /// The instances should be **all** the instances with MIR available in the current crate.
    pub fn analyze(
        &mut self,
        instances: Vec<Instance<'tcx>>,
        tcx: TyCtxt<'tcx>,
        param_env: ParamEnv<'tcx>,
    ) {
        let idx_insts = instances
            .into_iter()
            .map(|inst| {
                let idx = self.graph.add_node(CallGraphNode::WithBody(inst));
                (idx, inst)
            })
            .collect::<Vec<_>>();
        for (caller_idx, caller) in idx_insts {
            let body = tcx.instance_mir(caller.def);
            // Skip promoted src
            if body.source.promoted.is_some() {
                continue;
            }
            let mut collector = CallSiteCollector::new(caller, body, tcx, param_env);
            collector.visit_body(body);
            for (callee, location) in collector.finish() {
                let callee_idx = if let Some(callee_idx) = self.instance_to_index(&callee) {
                    callee_idx
                } else {
                    self.graph.add_node(CallGraphNode::WithoutBody(callee))
                };
                if let Some(edge_idx) = self.graph.find_edge(caller_idx, callee_idx) {
                    // Update edge weight.
                    self.graph.edge_weight_mut(edge_idx).unwrap().push(location);
                } else {
                    // Add edge if not exists.
                    self.graph.add_edge(caller_idx, callee_idx, vec![location]);
                }
            }
        }
    }

    /// Find the callsites (weight) on the edge from source to target.
    pub fn callsites(
        &self,
        source: InstanceId,
        target: InstanceId,
    ) -> Option<Vec<CallSiteLocation>> {
        let edge = self.graph.find_edge(source, target)?;
        self.graph.edge_weight(edge).cloned()
    }

    /// Find all the callers that call target
    pub fn callers(&self, target: InstanceId) -> Vec<InstanceId> {
        self.graph.neighbors_directed(target, Incoming).collect()
    }

    /// Find all simple paths from source to target.
    /// e.g., for one of the paths, `source --> instance1 --> instance2 --> target`,
    /// the return is [source, instance1, instance2, target].
    pub fn all_simple_paths(&self, source: InstanceId, target: InstanceId) -> Vec<Vec<InstanceId>> {
        algo::all_simple_paths::<Vec<_>, _>(&self.graph, source, target, 0, None)
            .collect::<Vec<_>>()
    }

    /// 根据调用序列删减无关调用
    /// 如果从 main 出发的某条执行路径上不包含 LockGuardId 中的函数 Id
    /// 那么此条路径上的函数不需要转换为网
    pub fn reduce_from_main(
        &mut self,
        start: InstanceId,
        end: InstanceId,
        lockguard: HashSet<InstanceId>,
    ) {
        let paths = self.all_simple_paths(source, target);

        for path in paths {
            if path.iter().any(|&x| lockguard.contains(x)) {
                continue;
            }

            let mut to_remove: HashSet<InstanceId> = path.iter().cloned().collect();
            to_remove.remove(&path.first());
            to_remove.remove(&path.last());

            // 删除节点
            let mut removal_list: Vec<_> = to_remove.into_iter().collect();
            removal_list.sort_by(|a, b| b.cmp(a));
            for node in removal_list {
                self.graph.remove_node(node);
            }
            self.graph.add_edge(
                &path.first(),
                &path.last(),
                vec![CallSiteLocation::Direct(())],
            )
        }
    }

    /// Print the callgraph in dot format.
    #[allow(dead_code)]
    pub fn dot(&self) -> String {
        format!(
            "{:?}",
            Dot::with_config(&self.graph, &[Config::EdgeNoLabel])
        )
    }
}

/// Visit Terminator and record callsites (callee + location).
struct CallSiteCollector<'a, 'tcx> {
    caller: Instance<'tcx>,
    body: &'a Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    param_env: ParamEnv<'tcx>,
    callsites: Vec<(Instance<'tcx>, CallSiteLocation)>,
}

impl<'a, 'tcx> CallSiteCollector<'a, 'tcx> {
    fn new(
        caller: Instance<'tcx>,
        body: &'a Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        param_env: ParamEnv<'tcx>,
    ) -> Self {
        Self {
            caller,
            body,
            tcx,
            param_env,
            callsites: Vec::new(),
        }
    }

    /// Consumes `CallSiteCollector` and returns its callsites when finished visiting.
    fn finish(self) -> impl IntoIterator<Item = (Instance<'tcx>, CallSiteLocation)> {
        self.callsites.into_iter()
    }
}

impl<'a, 'tcx> Visitor<'tcx> for CallSiteCollector<'a, 'tcx> {
    /// Resolve direct call.
    /// Inspired by rustc_mir/src/transform/inline.rs#get_valid_function_call.
    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        if let TerminatorKind::Call {
            ref func, ref args, ..
        } = terminator.kind
        {
            // Only after monomorphizing can Instance::resolve work
            // let func_ty = self.caller.instantiate_mir_and_normalize_erasing_regions(
            //     self.tcx,
            //     self.param_env,
            //     ty::EarlyBinder::bind(func.ty(&self.body.local_decls, self.tcx)),
            // );
            let func_ty = self.caller.subst_mir_and_normalize_erasing_regions(
                self.tcx,
                self.param_env,
                ty::EarlyBinder::bind(func.ty(self.body, self.tcx)),
            );
            if let ty::FnDef(def_id, substs) = *func_ty.kind() {
                // println!("func kind error");
                if let Some(callee) = Instance::resolve(self.tcx, self.param_env, def_id, substs)
                    .ok()
                    .flatten()
                {
                    self.callsites
                        .push((callee, CallSiteLocation::Direct(location)));
                }
                // println!("resolve instance error");
            }
        }
        self.super_terminator(terminator, location);
    }

    /// Find where the closure is defined rather than called,
    /// including the closure instance and the arg.
    ///
    /// e.g., let mut _20: [closure@src/main.rs:13:28: 16:6];
    ///
    /// _20 is of type Closure, but it is actually the arg that captures
    /// the variables in the defining function.
    fn visit_local_decl(&mut self, local: Local, local_decl: &LocalDecl<'tcx>) {
        // let func_ty = self.caller.instantiate_mir_and_normalize_erasing_regions(
        //     self.tcx,
        //     self.param_env,
        //     ty::EarlyBinder::bind(local_decl.ty),
        // );
        let func_ty = self.caller.subst_mir_and_normalize_erasing_regions(
            self.tcx,
            self.param_env,
            ty::EarlyBinder::bind(local_decl.ty),
        );
        if let TyKind::Closure(def_id, substs) = *func_ty.kind() {
            match self.body.local_kind(local) {
                LocalKind::Arg | LocalKind::ReturnPointer => {}
                _ => {
                    if let Some(callee_instance) =
                        Instance::resolve(self.tcx, self.param_env, def_id, substs)
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
