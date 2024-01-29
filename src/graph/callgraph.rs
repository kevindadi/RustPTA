//! 对crate里所有的instance分析，创建CallGraph
//! 是指针分析的基础
extern crate rustc_abi;

use petgraph::algo;
use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
use petgraph::visit::IntoNodeReferences;
use petgraph::Direction::{Incoming, Outgoing};
use petgraph::{Directed, Graph};

use rustc_middle::mir::visit::Visitor;
use rustc_middle::mir::{Body, Local, LocalDecl, LocalKind, Location, Terminator, TerminatorKind, Place};
use rustc_middle::ty::{self, Instance, ParamEnv, TyCtxt, TyKind, Ty};

use std::fs::File;
use std::io::Write;

/// 使用NodeIndex作为instance的id
pub type InstanceId = NodeIndex;

/// 调用点
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

/// 封装的Instance
/// WithBody：带函数体的实例；WithoutBody：不带函数体的实例
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

/// 函数调用图
/// Instance1--|[CallSite1, CallSite2]|-->Instance2
/// 代表Instance1在Callsite1和CallSite2处调用Instance2

pub struct CallGraph<'tcx> {
    pub graph: Graph<CallGraphNode<'tcx>, Vec<CallSiteLocation>, Directed>,
}

impl<'tcx> CallGraph<'tcx> {
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
        }
    }

    pub fn getNodes(&self) -> Vec<NodeIndex>{
        self.graph.node_indices().collect()
    }

    pub fn getFirstNode(&self) -> Option<NodeIndex>{
        self.graph.node_indices().next()
    }

    /// 返回函数的形参
    /// 在pointsto_inter中处理函数调用时用到
    pub fn getArgs(
        &self,
        callGraphNode:&CallGraphNode<'tcx>,
        tcx: TyCtxt<'tcx>,
    ) -> Vec<Place<'tcx>>{
        let mut formal_para_vec = Vec::new();
        match callGraphNode {
            CallGraphNode::WithBody(inst) =>{
                
                let callee_body = tcx.instance_mir(inst.def);
                let iter = callee_body.args_iter();
                for i in iter{
                    let p = Place::from(i);
                    formal_para_vec.push(p);
                }   
                formal_para_vec
            },
            CallGraphNode::WithoutBody(inst) =>{
                formal_para_vec
            }
        }
                 
    }

    /// 返回闭包的参数
    /// 在pointsto_inter中处理闭包时用到
    /// TODO：寻找对应闭包参数不够准确
    pub fn getClosureArg(
        &self,
        callGraphNode:&CallGraphNode<'tcx>,
        tcx: TyCtxt<'tcx>,
        ty:Ty<'tcx>,
    )-> Vec<Place<'tcx>>{
        let mut formal_para_vec = Vec::new();
        match callGraphNode {
            CallGraphNode::WithBody(inst)| CallGraphNode::WithoutBody(inst) =>{
                
                let closure_body = tcx.instance_mir(inst.def);
                for (local, local_ty) in closure_body.local_decls.iter_enumerated(){
                    // println!("closure_local{:?},closure_local_ty{:?}",local,local_ty);
                    if local_ty.ty.contains(ty) {
                        // TODO：找到了匹配的局部变量?? 不准确
                        let p = Place::from(local);
                        formal_para_vec.push(p);
                    }
                }
                formal_para_vec
            },
        }
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

    pub fn instance_to_CallGraphNode(&self, instance: &Instance<'tcx>)-> Option<&CallGraphNode<'tcx>>{
        if let Some(id) = self.instance_to_index(instance) {
            self.index_to_instance(id)
        }else{
            None
        }

    }

   
    /// 对所有的instance创建CallGraph
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
                    self.graph.edge_weight_mut(edge_idx).unwrap().push(location);
                } else {
                    self.graph.add_edge(caller_idx, callee_idx, vec![location]);
                }
            }
        }
    }

    /// 找出从source到target的边上的调用点
    pub fn callsites(
        &self,
        source: InstanceId,
        target: InstanceId,
    ) -> Option<Vec<CallSiteLocation>> {
        let edge = self.graph.find_edge(source, target)?;
        self.graph.edge_weight(edge).cloned()
    }

    /// 找出调用者
    pub fn callers(&self, target: InstanceId) -> Vec<InstanceId> {
        self.graph.neighbors_directed(target, Incoming).collect()
    }

    /// 找出被调用者
    pub fn callees(&self, source: InstanceId)-> Vec<InstanceId> {
        self.graph.neighbors_directed(source, Outgoing).collect()
    }

    /// 寻找从source到target的路径
    /// 比如路径source --> instance1 --> instance2 --> target
    /// 返回 [source, instance1, instance2, target]
    pub fn all_simple_paths(&self, source: InstanceId, target: InstanceId) -> Vec<Vec<InstanceId>> {
        algo::all_simple_paths::<Vec<_>, _>(&self.graph, source, target, 0, None)
            .collect::<Vec<_>>()
    }

    /// callgraph的dot输出
    #[allow(dead_code)]
    pub fn dot(&self) {
        let dot_string = format!("digraph G {{\n{:?}\n}}", Dot::with_config(&self.graph, &[Config::GraphContentOnly]));
        let output_file_path = "callgraph.dot";
        match File::create(output_file_path) {
            Ok(mut file) => {
                if let Err(err) = file.write_all(dot_string.as_bytes()) {
                    eprintln!("Failed to write to file: {}", err);
                } else {
                    println!("DOT representation saved to '{}'", output_file_path);
                }
            },
            Err(err) => {
                eprintln!("Failed to create file: {}", err);
            }
        }
    }
}

/// 遍历Terminator，记录callsites (callee + location).
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

    fn finish(self) -> impl IntoIterator<Item = (Instance<'tcx>, CallSiteLocation)> {
        self.callsites.into_iter()
    }
}

impl<'a, 'tcx> Visitor<'tcx> for CallSiteCollector<'a, 'tcx> { 
    /// 参考 rustc_mir/src/transform/inline.rs#get_valid_function_call.
    /// 寻找函数调用 TerminatorKind::Call
    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        if let TerminatorKind::Call { ref func, .. } = terminator.kind {
            let func_ty = func.ty(self.body, self.tcx);
            //monomorphizing 在Instance::resolve前必要的步骤
            let func_ty = self.caller.subst_mir_and_normalize_erasing_regions(
                self.tcx,
                self.param_env,
                ty::EarlyBinder::bind(func_ty),
            );
            
            if let ty::FnDef(def_id, substs) = *func_ty.kind() {
                if let Some(callee) = Instance::resolve(self.tcx, self.param_env, def_id, substs) 
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

    /// 寻找闭包定义
    /// let mut _18: {closure@src/main.rs:13:28: 13:35};
    /// _18 = {closure@src/main.rs:13:28: 13:35} { lock_a2: move _3, lock_b2: move _7 }
    /// 
    fn visit_local_decl(&mut self, local: Local, local_decl: &LocalDecl<'tcx>) {
        let func_ty = self.caller.subst_mir_and_normalize_erasing_regions(
            self.tcx,
            self.param_env,
            ty::EarlyBinder::bind(local_decl.ty),
        );
        if let TyKind::Closure(def_id, substs) = func_ty.kind() {
            match self.body.local_kind(local) {
                LocalKind::Arg | LocalKind::ReturnPointer => {}
                _ => {
                    if let Some(callee_instance) =
                        Instance::resolve(self.tcx, self.param_env, *def_id, substs)
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
