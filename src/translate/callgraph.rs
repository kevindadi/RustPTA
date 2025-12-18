use petgraph::Direction::Incoming;
use petgraph::algo;
use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
use petgraph::{Directed, Graph};

use std::collections::hash_map::RandomState;
use std::fs;
use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};
use rustc_hir::def_id::DefId;
use rustc_middle::mir::visit::Visitor;
use rustc_middle::mir::{
    Body, Local, LocalDecl, LocalKind, Location, Operand, Terminator, TerminatorKind,
};
use rustc_middle::ty::{self, GenericArgsRef, Instance, TyCtxt, TyKind, TypingEnv};
use rustc_span::source_map::Spanned;

pub type InstanceId = NodeIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadControlKind {
    Spawn,
    Join,
    ScopeSpawn,
    ScopeJoin,
    RayonJoin,
}

#[derive(Copy, Clone, Debug)]
pub enum CallSiteLocation {
    Direct(Location),
    ClosureDef(Local),
    ThreadControl {
        kind: ThreadControlKind,
        location: Location,
        destination: Option<Local>,
    },
}

impl CallSiteLocation {
    pub fn location(&self) -> Option<Location> {
        match self {
            Self::Direct(loc) => Some(*loc),
            Self::ThreadControl { location, .. } => Some(*location),
            _ => None,
        }
    }

    pub fn spawn_destination(&self) -> Option<Local> {
        match self {
            Self::ThreadControl {
                destination: Some(destination),
                kind: ThreadControlKind::Spawn | ThreadControlKind::ScopeSpawn,
                ..
            } => Some(*destination),
            _ => None,
        }
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
            CallGraphNode::WithBody(instance) | CallGraphNode::WithoutBody(instance) => instance,
        }
    }

    pub fn match_instance(&self, other: &Instance<'tcx>) -> bool {
        matches!(
            self,
            CallGraphNode::WithBody(instance) | CallGraphNode::WithoutBody(instance)
                if instance == other
        )
    }
}

pub struct CallGraph<'tcx> {
    pub graph: Graph<CallGraphNode<'tcx>, Vec<CallSiteLocation>, Directed>,

    pub spawn_calls: FxHashMap<DefId, FxHashMap<Local, FxHashSet<DefId>>>,
    instance_index: FxHashMap<Instance<'tcx>, InstanceId>,
}

impl<'tcx> CallGraph<'tcx> {
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
            spawn_calls: FxHashMap::default(),
            instance_index: FxHashMap::default(),
        }
    }

    pub fn format_spawn_calls(&self, tcx: TyCtxt<'tcx>) -> String {
        let mut output = String::from("Spawn calls in functions:\n");

        for (caller_id, spawn_set) in &self.spawn_calls {
            let caller_name = tcx.def_path_str(*caller_id);
            output.push_str(&format!("\nIn function {caller_name}:\n"));

            for (destination, callees) in spawn_set {
                output.push_str(&format!("  - Stored in _{}:\n", destination.index()));
                for callee in callees {
                    let closure_name = tcx.def_path_str(*callee);
                    output.push_str(&format!("      * {closure_name}\n"));
                }
            }
        }
        output
    }

    pub fn instance_to_index(&self, instance: &Instance<'tcx>) -> Option<InstanceId> {
        self.instance_index.get(instance).copied()
    }

    pub fn index_to_instance(&self, idx: InstanceId) -> Option<&CallGraphNode<'tcx>> {
        self.graph.node_weight(idx)
    }

    fn record_spawn_call(&mut self, caller: DefId, closure_idx: DefId, destination: Local) {
        self.spawn_calls
            .entry(caller)
            .or_default()
            .entry(destination)
            .or_default()
            .insert(closure_idx);
    }

    pub fn get_spawn_calls(&self, def_id: DefId) -> Option<&FxHashMap<Local, FxHashSet<DefId>>> {
        self.spawn_calls.get(&def_id)
    }

    pub fn analyze(&mut self, instances: Vec<Instance<'tcx>>, tcx: TyCtxt<'tcx>) {
        let idx_insts = instances
            .into_iter()
            .map(|inst| {
                let idx = self.insert_instance(CallGraphNode::WithBody(inst));
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
                let callee_idx = self.insert_instance(CallGraphNode::WithoutBody(callee));

                if let CallSiteLocation::ThreadControl {
                    kind: ThreadControlKind::Spawn | ThreadControlKind::ScopeSpawn,
                    destination: Some(destination),
                    ..
                } = location
                {
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
        algo::all_simple_paths::<Vec<_>, _, RandomState>(&self.graph, source, target, 0, None)
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

    fn insert_instance(&mut self, node: CallGraphNode<'tcx>) -> InstanceId {
        let instance = *node.instance();
        if let Some(idx) = self.instance_index.get(&instance) {
            if let CallGraphNode::WithBody(_) = &node {
                if let Some(weight) = self.graph.node_weight_mut(*idx) {
                    if matches!(weight, CallGraphNode::WithoutBody(_)) {
                        *weight = node;
                    }
                }
            }
            return *idx;
        }

        let idx = self.graph.add_node(node);
        self.instance_index.insert(instance, idx);
        idx
    }
}

struct CallSiteCollector<'a, 'tcx> {
    caller: Instance<'tcx>,
    body: &'a Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    callsites: Vec<(Instance<'tcx>, CallSiteLocation)>,
    typing_env: TypingEnv<'tcx>,
}

impl<'a, 'tcx> CallSiteCollector<'a, 'tcx> {
    fn new(caller: Instance<'tcx>, body: &'a Body<'tcx>, tcx: TyCtxt<'tcx>) -> Self {
        let typing_env = TypingEnv::post_analysis(tcx, caller.def_id());
        Self {
            caller,
            body,
            tcx,
            callsites: Vec::new(),
            typing_env,
        }
    }

    fn finish(self) -> impl IntoIterator<Item = (Instance<'tcx>, CallSiteLocation)> {
        self.callsites.into_iter()
    }

    fn resolve_instance(
        &self,
        def_id: DefId,
        substs: GenericArgsRef<'tcx>,
    ) -> Option<Instance<'tcx>> {
        Instance::try_resolve(self.tcx, self.typing_env, def_id, substs)
            .ok()
            .flatten()
    }

    fn operand_closure_instance(
        &self,
        operand: &Operand<'tcx>,
        substs: GenericArgsRef<'tcx>,
    ) -> Option<Instance<'tcx>> {
        let closure_ty = match operand {
            Operand::Move(place) | Operand::Copy(place) => place.ty(self.body, self.tcx).ty,
            Operand::Constant(constant) => constant.ty(),
        };

        match *closure_ty.kind() {
            ty::Closure(def_id, _) | ty::FnDef(def_id, _) => self.resolve_instance(def_id, substs),
            _ => None,
        }
    }

    fn handle_spawn_call(
        &mut self,
        args: &[Spanned<Operand<'tcx>>],
        destination: Local,
        location: Location,
        substs: GenericArgsRef<'tcx>,
        kind: ThreadControlKind,
    ) -> bool {
        for operand in args.iter().take(2) {
            if let Some(callee) = self.operand_closure_instance(&operand.node, substs) {
                self.callsites.push((
                    callee,
                    CallSiteLocation::ThreadControl {
                        kind,
                        location,
                        destination: Some(destination),
                    },
                ));
                return true;
            }
        }
        false
    }

    fn handle_rayon_join(
        &mut self,
        args: &[Spanned<Operand<'tcx>>],
        substs: GenericArgsRef<'tcx>,
        location: Location,
    ) -> bool {
        let mut recorded = false;
        for operand in args {
            if let Some(callee) = self.operand_closure_instance(&operand.node, substs) {
                self.callsites.push((
                    callee,
                    CallSiteLocation::ThreadControl {
                        kind: ThreadControlKind::RayonJoin,
                        location,
                        destination: None,
                    },
                ));
                recorded = true;
            }
        }
        recorded
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
            let func_ty = self.caller.instantiate_mir_and_normalize_erasing_regions(
                self.tcx,
                self.typing_env,
                ty::EarlyBinder::bind(func.ty(self.body, self.tcx)),
            );

            if let ty::FnDef(def_id, substs) = *func_ty.kind() {
                let fn_path = self.tcx.def_path_str(def_id);
                if let Some(control_kind) = classify_thread_control(&fn_path) {
                    match control_kind {
                        ThreadControlKind::Spawn | ThreadControlKind::ScopeSpawn => {
                            if self.handle_spawn_call(
                                args.as_ref(),
                                destination.local,
                                location,
                                substs,
                                control_kind,
                            ) {
                                return;
                            }
                        }
                        ThreadControlKind::RayonJoin => {
                            if self.handle_rayon_join(args.as_ref(), substs, location) {
                                return;
                            }
                        }
                        ThreadControlKind::Join | ThreadControlKind::ScopeJoin => {
                            if let Some(callee) = self.resolve_instance(def_id, substs) {
                                self.callsites.push((
                                    callee,
                                    CallSiteLocation::ThreadControl {
                                        kind: control_kind,
                                        location,
                                        destination: Some(destination.local),
                                    },
                                ));
                            }
                            return;
                        }
                    }
                }

                if let Some(callee) = self.resolve_instance(def_id, substs) {
                    self.callsites
                        .push((callee, CallSiteLocation::Direct(location)));
                }
            }
        }
        self.super_terminator(terminator, location);
    }

    fn visit_local_decl(&mut self, local: Local, local_decl: &LocalDecl<'tcx>) {
        let func_ty = self.caller.instantiate_mir_and_normalize_erasing_regions(
            self.tcx,
            self.typing_env,
            ty::EarlyBinder::bind(local_decl.ty),
        );
        if let TyKind::Closure(def_id, substs) = *func_ty.kind() {
            match self.body.local_kind(local) {
                LocalKind::Arg | LocalKind::ReturnPointer => {}
                _ => {
                    if let Some(callee_instance) = self.resolve_instance(def_id, substs) {
                        self.callsites
                            .push((callee_instance, CallSiteLocation::ClosureDef(local)));
                    }
                }
            }
        }
        self.super_local_decl(local, local_decl);
    }
}

fn classify_thread_control(fn_path: &str) -> Option<ThreadControlKind> {
    if RAYON_JOIN_PATTERNS
        .iter()
        .any(|pattern| fn_path.contains(pattern))
    {
        return Some(ThreadControlKind::RayonJoin);
    }

    if THREAD_SCOPE_SPAWN_PATTERNS
        .iter()
        .any(|pattern| fn_path.contains(pattern))
    {
        return Some(ThreadControlKind::ScopeSpawn);
    }

    if THREAD_SCOPE_JOIN_PATTERNS
        .iter()
        .any(|pattern| fn_path.contains(pattern))
    {
        return Some(ThreadControlKind::ScopeJoin);
    }

    if THREAD_SPAWN_PATTERNS
        .iter()
        .any(|pattern| fn_path.contains(pattern))
    {
        return Some(ThreadControlKind::Spawn);
    }

    if THREAD_JOIN_PATTERNS
        .iter()
        .any(|pattern| fn_path.contains(pattern))
    {
        return Some(ThreadControlKind::Join);
    }

    None
}

const THREAD_SPAWN_PATTERNS: &[&str] = &[
    "std::thread::spawn",
    "tokio::task::spawn",
    "tokio::runtime::Runtime::spawn",
    "async_std::task::spawn",
    "smol::Task::spawn",
    "smol::spawn",
    "rayon::spawn",
];

const THREAD_JOIN_PATTERNS: &[&str] = &[
    "std::thread::JoinHandle::join",
    "std::thread::JoinHandle::try_join",
    "tokio::task::JoinHandle::await",
    "tokio::task::JoinHandle::blocking_on",
];

const THREAD_SCOPE_SPAWN_PATTERNS: &[&str] = &[
    "std::thread::scope::Scope::spawn",
    "crossbeam::scope::Scope::spawn",
    "rayon::scope::Scope::spawn",
];

const THREAD_SCOPE_JOIN_PATTERNS: &[&str] = &[
    "std::thread::scope::Scope::join",
    "crossbeam::scope::Scope::join",
    "rayon::scope::Scope::join",
];

const RAYON_JOIN_PATTERNS: &[&str] = &["rayon_core::join", "rayon::join"];
