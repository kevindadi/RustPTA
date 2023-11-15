//!过程间 上下敏感的指针分析
//! 改进1.构造了过程间的 Constraint Graph，即所有instance
//! 改进2.形参和实参间的指向关系
//! 改进3.a=&b 如果b是Place类型 不是Alloc类型
//! 
//! 1. _8 = &((*_1).0)
//! 2.
extern crate rustc_hash;
extern crate rustc_hir;
extern crate rustc_index;


use std::cmp::{Ordering, PartialOrd};
use std::collections::VecDeque;

use rustc_hash::{FxHashMap, FxHashSet};
use rustc_hir::def_id::DefId;
use rustc_middle::mir::visit::Visitor;
use rustc_middle::mir::{
    Body, Constant, ConstantKind, Local, Location, Operand, Place, PlaceElem, PlaceRef,
    ProjectionElem, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,LocalDecl,LocalInfo,LocalKind,
};


use rustc_middle::ty::{self,Instance, TyCtxt, TyKind,ParamEnv};
use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::{Directed, Direction, Graph};
use petgraph::visit::IntoNodeReferences;

use crate::concurrency::atomic::is_atomic_ptr_store;
use crate::concurrency::locks::LockGuardId;
use crate::graph::callgraph::{CallGraph, CallGraphNode, CallSiteLocation, InstanceId, self};
use crate::memory::ownership;
use std::fs::File;
use std::io::Write;

/// Field-sensitive intra-procedural Andersen pointer analysis.
/// <https://helloworld.pub/program-analysis-andersen-pointer-analysis-algorithm-based-on-svf.html>
/// 1. collect constraints from MIR to build a `ConstraintGraph`
/// 2. adopt a fixed-point algorithm to update `ConstraintGraph` and points-to info
///
/// There are several changes:
/// 1. Use a place to represent a memroy cell.
/// 2. Create an Alloc node for each place and let the place points to it.
/// 3. Distinguish local places with global ones (denoted as Constant).
/// 4. Treat special functions by names or signatures (e.g., Arc::clone).
/// 5. Interproc methods: Use parameters' type info to guide the analysis heuristically (simple but powerful).
/// 6. Interproc closures: Track the upvars of closures in the functions defining the closures (restricted).
pub struct Andersen<'a,'tcx> {
    tcx: TyCtxt<'tcx>,
    pts: PointsToMap<'tcx>,
    callgraph: &'a CallGraph<'tcx>,
}

pub type PointsToMap<'tcx> = FxHashMap<ConstraintNode<'tcx>, FxHashSet<ConstraintNode<'tcx>>>;

impl<'a, 'tcx> Andersen<'a,'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, callgraph: &'a CallGraph<'tcx>,) -> Self {
        Self {
            tcx,
            pts: Default::default(),
            callgraph,
        }
    }

    pub fn analyze(
        &mut self,
        param_env: ParamEnv<'tcx>,
        instances: Vec<&'a Instance<'tcx>>,
    ) {
        // if let Some(index) =self.callgraph.getFirstNode(){
            
        //     if let Some(node)=self.callgraph.index_to_instance(index){
                // let instance_first = *node.instance();
                let instance_first = instances[0];
                let body_first = self.tcx.instance_mir(instance_first.def);
                let mut consGCollector = ConstraintGraphCollector::new(instance_first, body_first, self.tcx,self.callgraph,param_env);
                consGCollector.analyze(instances);
                let mut graph = consGCollector.finish();

                // graph.dot();

                let mut worklist = VecDeque::new();
                // alloc: place = alloc
                for node in graph.nodes() {
                    match node {
                        ConstraintNode::Place(place,instance_id) => {
                            graph.add_alloc(place,instance_id);
                        }
                        ConstraintNode::Constant(constant,instance_id) => {
                            graph.add_constant(constant,instance_id);
                            // For constant C, track *C.
                            worklist.push_back(ConstraintNode::ConstantDeref(constant,instance_id));
                        }
                        _ => {}
                    }
                    worklist.push_back(node);
                }
        
                // address: target = &source
                for (source, target, weight) in graph.edges() {
                    if weight == ConstraintEdge::Address {
                        if matches!(source, ConstraintNode::Place(_,_)) {
                            // _16 = &_3 需要pts(_16) = pts(_3)
                            // if let Some(source_pts) = self.pts.get(&source) {
                            //     self.pts.insert(target, source_pts.clone());
                            // }
                            if graph.insert_edge(source, target, ConstraintEdge::AddressCopy) {
                                worklist.push_back(target);
                            }
                            // println!("address: target = &source,target{:?},source{:?}",target,source);
                        }else{
                            self.pts.entry(target).or_default().insert(source);//
                            worklist.push_back(target);
                        }
                        
                    }
                }
                
                while let Some(node) = worklist.pop_front() {
                    if !self.pts.contains_key(&node) {
                        continue;
                    }
                    for o in self.pts.get(&node).unwrap() {
                        // store: *node = source
                        for source in graph.store_sources(&node) {
                            if graph.insert_edge(source, *o, ConstraintEdge::Copy) {
                                worklist.push_back(source);
                            }
                        }
                        // load: target = *node
                        for target in graph.load_targets(&node) {
                            if graph.insert_edge(*o, target, ConstraintEdge::Copy) {
                                worklist.push_back(*o);
                            }
                        }
                    }
                    // alias_copy: target = &X; X = ptr::read(node)
                    for target in graph.alias_copy_targets(&node) {
                        if graph.insert_edge(node, target, ConstraintEdge::Copy) {
                            worklist.push_back(node);
                        }
                    }
                    // address_copy: a=&b b是Place类型 不是Alloc类型 target = &node
                    for target in graph.address_copy_targets(&node) {
                        if self.equal_pts(&target, &node) { //应该是直接等于
                            worklist.push_back(target);
                        }
                    }
                    //field_address _1.0 target <-- node _1
                    for target in graph.field_address_targets(&node) {
                        //根据node 创建 alloc域节点
                        let source_nodes = self.pts.get(&node).unwrap().clone();
                        let fnodes = graph.getFieldAllocNodes(source_nodes,target);
                        let len1 = self.pts.get(&target).unwrap().len();
                        for fnode in fnodes{
                            // println!("Field_ALloc:{:?}",fnode);
                            graph.get_or_insert_node(fnode);
                            graph.insert_edge(fnode, target, ConstraintEdge::Address);
                            self.pts.entry(target).or_default().insert(fnode);//    
                        }
                        let len2 = self.pts.get(&target).unwrap().len();
                        if len1 != len2{
                            worklist.push_back(target);
                        }
                    }
                    // copy: target = node
                    for target in graph.copy_targets(&node) {
                        if self.union_pts(&target, &node) {
                            worklist.push_back(target);
                        }
                    }
                }
                
                graph.dot();
        //     }else{}
            
        // }else{}
        
    }

    /// pts(target) = pts(target) U pts(source), return true if pts(target) changed
    fn union_pts(&mut self, target: &ConstraintNode<'tcx>, source: &ConstraintNode<'tcx>) -> bool {
        // skip Alloc target
        if matches!(target, ConstraintNode::Alloc(_,_)) {
            return false;
        }
        let old_len = self.pts.get(target).unwrap().len();
        let source_pts = self.pts.get(source).unwrap().clone();
        let target_pts = self.pts.get_mut(target).unwrap();
        target_pts.extend(source_pts.into_iter());
        old_len != target_pts.len()
    }

   fn equal_pts(&mut self, target: &ConstraintNode<'tcx>, source: &ConstraintNode<'tcx>) -> bool {
        let source_pts = self.pts.get(source).unwrap();
        let target_pts = self.pts.get(target).unwrap();
        if source_pts.len() == target_pts.len() {
            for e in source_pts.iter() {
                if !target_pts.contains(e) {
                    return false;
                }
            }
        }
        self.pts.insert(*target, source_pts.clone());
        true
    } 

    /// target <-- source
   

    pub fn finish(self) -> FxHashMap<ConstraintNode<'tcx>, FxHashSet<ConstraintNode<'tcx>>> {
        self.pts
    }

    pub fn alias(
        &mut self, 
        aid1: AliasId, 
        aid2: AliasId,
        param_env: ParamEnv<'tcx>,
    ) -> ApproximateAliasKind {

        
        let AliasId {
            instance_id: id1,
            local: local1,
        } = aid1;
        let AliasId {
            instance_id: id2,
            local: local2,
        } = aid2;

        let instance1 = self
            .callgraph
            .index_to_instance(id1)
            .map(CallGraphNode::instance);
        let instance2 = self
            .callgraph
            .index_to_instance(id2)
            .map(CallGraphNode::instance);
        let node1 = ConstraintNode::Place(Place::from(local1).as_ref(),id1);
        let node2 = ConstraintNode::Place(Place::from(local2).as_ref(),id2);
        let set1 = self.pts.get(&node1);
        let set2 = self.pts.get(&node2);
        if let (Some(set1), Some(set2)) = (set1, set2) {
            println!("\nNODE:{:?}",node1);
            for s1 in set1{
                println!("{:?}",s1);
            }
            println!("\nNODE:{:?}",node2);
            for s2 in set2{
                println!("{:?}",s2);
            }
            if set2.contains(&node1) || set1.contains(&node2) {
                ApproximateAliasKind::Possibly
            } else {
                let intersection = set1.intersection(set2);
                if intersection.count() > 0 {
                    ApproximateAliasKind::Possibly
                } else {
                    ApproximateAliasKind::Unlikely
                }
            }
        } else if let Some(set2) = set2 {
            if set2.contains(&node1) {
                ApproximateAliasKind::Possibly
            } else {
                ApproximateAliasKind::Unlikely
            }
        } else if let Some(set1) = set1 {
            if set1.contains(&node2) {
                ApproximateAliasKind::Possibly
            } else {
                ApproximateAliasKind::Unlikely
            }
        }
        else {
            ApproximateAliasKind::Unlikely
        }
        
    }
    
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AliasId {
    pub instance_id: InstanceId,
    pub local: Local,
}
/// Basically, `AliasId` and `LockGuardId` share the same info.
impl std::convert::From<LockGuardId> for AliasId {
    fn from(lockguard_id: LockGuardId) -> Self {
        Self {
            instance_id: lockguard_id.instance_id,
            local: lockguard_id.local,
        }
    }
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ApproximateAliasKind {
    Probably,
    Possibly,
    Unlikely,
    Unknown,
}
impl ApproximateAliasKind {
    pub fn to_string(&self) -> String {
        match self {
            ApproximateAliasKind::Probably => String::from("Probably"),
            ApproximateAliasKind::Possibly => String::from("Possibly"),
            ApproximateAliasKind::Unlikely => String::from("Unlikely"),
            ApproximateAliasKind::Unknown => String::from("Unknown"),
        }
    }
}

/// Probably > Possibly > Unlikey > Unknown
impl PartialOrd for ApproximateAliasKind {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        use ApproximateAliasKind::*;
        match (*self, *other) {
            (Probably, Probably)
            | (Possibly, Possibly)
            | (Unlikely, Unlikely)
            | (Unknown, Unknown) => Some(Ordering::Equal),
            (Probably, _) | (Possibly, Unlikely) | (Possibly, Unknown) | (Unlikely, Unknown) => {
                Some(Ordering::Greater)
            }
            (_, Probably) | (Unlikely, Possibly) | (Unknown, Possibly) | (Unknown, Unlikely) => {
                Some(Ordering::Less)
            }
        }
    }
}

/// `ConstraintNode` represents a memory cell, denoted by `Place` in MIR.
/// A `Place` encompasses `Local` and `[ProjectionElem]`, `ProjectionElem`
/// can be a `Field`, `Index`, etc.
/// Since there is no `Alloc` in MIR, we cannot use locations of `Alloc`
/// to uniquely identify the allocation of a memory cell.
/// Instead, we use `Place` itself to represent its allocation,
/// namely, forall Place(p), Alloc(p)--|address|-->Place(p).
/// `Constant` appears on right-hand in assignments like `Place = Constant(c)`.
/// To enable the propagtion of points-to info for `Constant`,
/// we introduce `ConstantDeref` to denote the points-to node of `Constant`,
/// namely, forall Constant(c), Constant(c)--|address|-->ConstantDeref(c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstraintNode<'tcx> {
    Alloc(PlaceRef<'tcx>,InstanceId),
    Place(PlaceRef<'tcx>,InstanceId),
    Constant(ConstantKind<'tcx>,InstanceId),
    ConstantDeref(ConstantKind<'tcx>,InstanceId),
}


/// The assignments in MIR with default `mir-opt-level` (level 1) are simplified
/// to the following four kinds:
///
/// | Edge   | Assignment | Constraint |
/// | ------ | ---------- | ----------
/// | Address| a = &b     | pts(a)∋b
/// | Copy   | a = b      | pts(a)⊇pts(b)
/// | Load   | a = *b     | ∀o∈pts(b), pts(a)⊇pts(o)
/// | Store  | *a = b     | ∀o∈pts(a), pts(o)⊇pts(b)
///
/// Note that other forms like a = &((*b).0) exists but is uncommon.
/// This is the case when b is an arg. I just treat (*b).0
/// as the mem cell and do not further dereference it.
/// I also introduce `AliasCopy` edge to represent x->y
/// for y=Arc::clone(x) and y=ptr::read(x),
/// where x--|copy|-->pointers of y
/// and x--|load|-->y (y=*x)
#[derive(Debug, Clone, Copy, PartialEq, Eq,Hash)]
enum ConstraintEdge {
    Address,
    Copy,
    Load,
    Store,
    AliasCopy, // Special: y=Arc::clone(x) or y=ptr::read(x)
    AddressCopy,// a=&b Place(b)
    FieldAddress,//a = &((*b).0)
}

enum AccessPattern<'tcx> {
    Ref(PlaceRef<'tcx>),
    Indirect(PlaceRef<'tcx>),
    Direct(PlaceRef<'tcx>),
    Constant(ConstantKind<'tcx>),
    Field(PlaceRef<'tcx>),
}

#[derive(Default)]
struct ConstraintGraph<'tcx> {
    graph: Graph<ConstraintNode<'tcx>, ConstraintEdge, Directed>,
    node_map: FxHashMap<ConstraintNode<'tcx>, NodeIndex>,
    edges:FxHashSet<(NodeIndex,NodeIndex,ConstraintEdge)>,
}

impl<'tcx> ConstraintGraph<'tcx> {
    pub fn get_or_insert_node(&mut self, node: ConstraintNode<'tcx>) -> NodeIndex {
        if let Some(idx) = self.node_map.get(&node) {
            *idx
        } else {
            let idx = self.graph.add_node(node);//添加节点
            self.node_map.insert(node, idx);
            idx
        }
    }

    pub fn get_node(&self, node: &ConstraintNode<'tcx>) -> Option<NodeIndex> {
        self.node_map.get(node).copied()
    }
    pub fn getFieldAllocNodes(&mut self,source_nodes:FxHashSet<ConstraintNode<'tcx>>,target:ConstraintNode<'tcx>) -> FxHashSet<ConstraintNode<'tcx>>{
        let mut fieldAllocNodes = FxHashSet::default();
        for s in source_nodes{
            match (s,target) {
                (ConstraintNode::Alloc(place_ref1, instance_id1),ConstraintNode::Place(place_ref2,instance_id2 )) => {
                    match place_ref2 {
                        PlaceRef {
                            local: l,
                            projection: p,
                        } => {
                            if let Some(new_local) = place_ref1.as_local(){
                                let new_node = ConstraintNode::Alloc(PlaceRef {
                                    local: new_local,
                                    projection: p,
                                }, instance_id1);
                
                                fieldAllocNodes.insert(new_node);
                            }
                        }  
                        _ => {},
                    }  
                }
                _=> {}
            }
        }
        fieldAllocNodes
    }
    pub fn add_edge_check(&mut self,rhs:NodeIndex,lhs:NodeIndex,edgeTy:ConstraintEdge){
        if !self.edges.contains(&(rhs,lhs,edgeTy)){
            self.edges.insert((rhs,lhs,edgeTy));
            self.graph.add_edge(rhs, lhs, edgeTy);
        }
    }
    
    fn add_alloc(&mut self, place: PlaceRef<'tcx>, instanceid:InstanceId) {
        let lhs = ConstraintNode::Place(place,instanceid);
        let rhs = ConstraintNode::Alloc(place,instanceid);
        let lhs = self.get_or_insert_node(lhs);
        let rhs = self.get_or_insert_node(rhs);
        self.add_edge_check(rhs, lhs, ConstraintEdge::Address);
        
    }

    fn add_constant(&mut self, constant: ConstantKind<'tcx>, instanceid:InstanceId) {
        let lhs = ConstraintNode::Constant(constant,instanceid);
        let rhs = ConstraintNode::ConstantDeref(constant,instanceid);
        let lhs = self.get_or_insert_node(lhs);
        let rhs = self.get_or_insert_node(rhs);
        self.add_edge_check(rhs, lhs, ConstraintEdge::Address);
        // For a constant C, there may be deref like *C, **C, ***C, ... in a real program.
        // For simplicity, we only track *C, and treat **C, ***C, ... the same as *C.
        self.add_edge_check(rhs, rhs, ConstraintEdge::Address);
    }

    fn add_address(&mut self, lhs: PlaceRef<'tcx>, rhs: PlaceRef<'tcx>, instanceid:InstanceId) {
        let lhs = ConstraintNode::Place(lhs,instanceid);
        let rhs = ConstraintNode::Place(rhs,instanceid);
        let lhs = self.get_or_insert_node(lhs);
        let rhs = self.get_or_insert_node(rhs);
        self.add_edge_check(rhs, lhs, ConstraintEdge::Address);
    }

    fn add_copy(&mut self, lhs: PlaceRef<'tcx>, rhs: PlaceRef<'tcx>, instanceid:InstanceId) {
        let lhs = ConstraintNode::Place(lhs,instanceid);
        let rhs = ConstraintNode::Place(rhs,instanceid);
        let lhs = self.get_or_insert_node(lhs);
        let rhs = self.get_or_insert_node(rhs);
        self.add_edge_check(rhs, lhs, ConstraintEdge::Copy);
    }

    fn add_copy_constant(&mut self, lhs: PlaceRef<'tcx>, rhs: ConstantKind<'tcx>, instanceid:InstanceId) {
        let lhs = ConstraintNode::Place(lhs,instanceid);
        let rhs = ConstraintNode::Constant(rhs,instanceid);
        let lhs = self.get_or_insert_node(lhs);
        let rhs = self.get_or_insert_node(rhs);
        self.add_edge_check(rhs, lhs, ConstraintEdge::Copy);
    }

    fn add_load(&mut self, lhs: PlaceRef<'tcx>, rhs: PlaceRef<'tcx>, instanceid:InstanceId) {
        let lhs = ConstraintNode::Place(lhs,instanceid);
        let rhs = ConstraintNode::Place(rhs,instanceid);
        let lhs = self.get_or_insert_node(lhs);
        let rhs = self.get_or_insert_node(rhs);
        self.add_edge_check(rhs, lhs, ConstraintEdge::Load);
    }

    fn add_store(&mut self, lhs: PlaceRef<'tcx>, rhs: PlaceRef<'tcx>, instanceid:InstanceId) {
        let lhs = ConstraintNode::Place(lhs,instanceid);
        let rhs = ConstraintNode::Place(rhs,instanceid);
        let lhs = self.get_or_insert_node(lhs);
        let rhs = self.get_or_insert_node(rhs);
        self.add_edge_check(rhs, lhs, ConstraintEdge::Store);
    }
    fn add_call_arg(&mut self, real: PlaceRef<'tcx>, formal: PlaceRef<'tcx>, call_instanceid:InstanceId,callee_instanceid:InstanceId) {
        let real = ConstraintNode::Place(real,call_instanceid);
        let formal = ConstraintNode::Place(formal,callee_instanceid);
        let real = self.get_or_insert_node(real);
        let formal = self.get_or_insert_node(formal);
        self.add_edge_check(real, formal, ConstraintEdge::Copy);
        
    }
    fn add_store_constant(&mut self, lhs: PlaceRef<'tcx>, rhs: ConstantKind<'tcx>, instanceid:InstanceId) {
        let lhs = ConstraintNode::Place(lhs,instanceid);
        let rhs = ConstraintNode::Constant(rhs,instanceid);
        let lhs = self.get_or_insert_node(lhs);
        let rhs = self.get_or_insert_node(rhs);
        self.add_edge_check(rhs, lhs, ConstraintEdge::Store);
    }

    fn add_alias_copy(&mut self, lhs: PlaceRef<'tcx>, rhs: PlaceRef<'tcx>, instanceid:InstanceId) {
        let lhs = ConstraintNode::Place(lhs,instanceid);
        let rhs = ConstraintNode::Place(rhs,instanceid);
        let lhs = self.get_or_insert_node(lhs);
        let rhs = self.get_or_insert_node(rhs);
        self.add_edge_check(rhs, lhs, ConstraintEdge::AliasCopy);
    }

    fn add_field_address(&mut self, lhs: PlaceRef<'tcx>, rhs: PlaceRef<'tcx>, instanceid:InstanceId){
        // _8 = &((*_1).0
        //lhs:_8; rhs:((*_1).0: std::sync::Arc<std::sync::Mutex<i32>>)
        if let Some(field_var) = self.get_field_var(rhs){
            let lhs = ConstraintNode::Place(lhs,instanceid);
            let rhs = ConstraintNode::Place(rhs,instanceid);
            let field_var = ConstraintNode::Place(field_var,instanceid);
            let lhs = self.get_or_insert_node(lhs);
            let rhs = self.get_or_insert_node(rhs);
            let field_var = self.get_or_insert_node(field_var);
            self.add_edge_check(field_var, rhs, ConstraintEdge::FieldAddress); // _1 ---> _1.0 
            self.add_edge_check(rhs, lhs, ConstraintEdge::Address);
        }
    }

    fn get_field_var(& mut self,rvalue:PlaceRef<'tcx>) -> Option<PlaceRef<'tcx>>{
        match rvalue {
            PlaceRef {
                local: l,
                projection: [..,ProjectionElem::Field(_, _),],
            } => Some(Place::from(l).as_ref()),  
            _ => None,
        }
    }
    fn nodes(&self) -> Vec<ConstraintNode<'tcx>> {
        self.node_map.keys().copied().collect::<_>()
    }

    fn edges(&self) -> Vec<(ConstraintNode<'tcx>, ConstraintNode<'tcx>, ConstraintEdge)> {
        let mut v = Vec::new();
        for edge in self.graph.edge_references() {
            let source = self.graph.node_weight(edge.source()).copied().unwrap();
            let target = self.graph.node_weight(edge.target()).copied().unwrap();
            let weight = *edge.weight();
            v.push((source, target, weight));
        }
        v
    }

    /// *lhs = ?
    /// ?--|store|-->lhs
    fn store_sources(&self, lhs: &ConstraintNode<'tcx>) -> Vec<ConstraintNode<'tcx>> {
        let lhs = self.get_node(lhs).unwrap();
        let mut sources = Vec::new();
        for edge in self.graph.edges_directed(lhs, Direction::Incoming) {
            if *edge.weight() == ConstraintEdge::Store {
                let source = self.graph.node_weight(edge.source()).copied().unwrap();
                sources.push(source);
            }
        }
        sources
    }

    /// ? = *rhs
    /// rhs--|load|-->?
    fn load_targets(&self, rhs: &ConstraintNode<'tcx>) -> Vec<ConstraintNode<'tcx>> {
        let rhs = self.get_node(rhs).unwrap();
        let mut targets = Vec::new();
        for edge in self.graph.edges_directed(rhs, Direction::Outgoing) {
            if *edge.weight() == ConstraintEdge::Load {
                let target = self.graph.node_weight(edge.target()).copied().unwrap();
                targets.push(target);
            }
        }
        targets
    }

    /// ? = rhs
    /// rhs--|copy|-->?
    fn copy_targets(&self, rhs: &ConstraintNode<'tcx>) -> Vec<ConstraintNode<'tcx>> {
        let rhs = self.get_node(rhs).unwrap();
        let mut targets = Vec::new();
        for edge in self.graph.edges_directed(rhs, Direction::Outgoing) {
            if *edge.weight() == ConstraintEdge::Copy {
                let target = self.graph.node_weight(edge.target()).copied().unwrap();
                targets.push(target);
            }
        }
        targets
    }

    /// a = &b b是Place 不是Alloc  ?=&rhs
    fn address_copy_targets(&self, rhs: &ConstraintNode<'tcx>) -> Vec<ConstraintNode<'tcx>> {
        let rhs = self.get_node(rhs).unwrap();
        let mut targets = Vec::new();
        for edge in self.graph.edges_directed(rhs, Direction::Outgoing) {
            if *edge.weight() == ConstraintEdge::AddressCopy {
                let target = self.graph.node_weight(edge.target()).copied().unwrap();
                targets.push(target);
            }
        }
        targets
    }

    /// (*_1).0 <--|FieldAddress|-- _1
    fn field_address_targets(&self, rhs: &ConstraintNode<'tcx>) -> Vec<ConstraintNode<'tcx>> {
        let rhs = self.get_node(rhs).unwrap();
        let mut targets = Vec::new();
        for edge in self.graph.edges_directed(rhs, Direction::Outgoing) {
            if *edge.weight() == ConstraintEdge::FieldAddress {
                let target = self.graph.node_weight(edge.target()).copied().unwrap();
                targets.push(target);
            }
        }
        targets
    }
    /// X = Arc::clone(rhs) or X = ptr::read(rhs)
    /// ? = &X
    ///
    /// rhs--|alias_copy|-->X,
    /// X--|address|-->?
    fn alias_copy_targets(&self, rhs: &ConstraintNode<'tcx>) -> Vec<ConstraintNode<'tcx>> {
        let rhs = self.get_node(rhs).unwrap();
        self.graph
            .edges_directed(rhs, Direction::Outgoing)
            .filter_map(|edge| {
                if *edge.weight() == ConstraintEdge::AliasCopy {
                    Some(edge.target())
                } else {
                    None
                }
            })
            .fold(Vec::new(), |mut acc, copy_alias_target| {
                let address_targets = self
                    .graph
                    .edges_directed(copy_alias_target, Direction::Outgoing)
                    .filter_map(|edge| {
                        if *edge.weight() == ConstraintEdge::Address {
                            Some(self.graph.node_weight(edge.target()).copied().unwrap())
                        } else {
                            None
                        }
                    });
                acc.extend(address_targets);
                acc
            })
    }

    /// if edge `from--|weight|-->to` not exists,
    /// then add the edge and return true
    fn insert_edge(
        &mut self,
        from: ConstraintNode<'tcx>,
        to: ConstraintNode<'tcx>,
        weight: ConstraintEdge,
    ) -> bool {
        let from = self.get_node(&from).unwrap();
        let to = self.get_node(&to).unwrap();
        if let Some(edge) = self.graph.find_edge(from, to) {
            if let Some(w) = self.graph.edge_weight(edge) {
                if *w == weight {
                    return false;
                }
            }
        }
        self.add_edge_check(from, to, weight);
        true
    }

    /// Print the callgraph in dot format.
    #[allow(dead_code)]
    pub fn dot(&self) {
        let dot_string = format!("digraph G {{\n{:?}\n}}", Dot::with_config(&self.graph, &[Config::GraphContentOnly]));
        let output_file_path = "ConstraintGraph_pt.dot";
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

/// Generate `ConstraintGraph` by visiting MIR body.
struct ConstraintGraphCollector<'a, 'tcx> {
    instance: &'a Instance<'tcx>,
    body: &'a Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    graph: ConstraintGraph<'tcx>,
    callgraph: &'a CallGraph<'tcx>,
    worklist_instance: VecDeque<NodeIndex>,
    param_env: ParamEnv<'tcx>,
}

impl<'a, 'tcx> ConstraintGraphCollector<'a, 'tcx> {
    fn new(instance: &'a Instance<'tcx>,body: &'a Body<'tcx>, tcx: TyCtxt<'tcx>,callgraph: &'a CallGraph<'tcx>,param_env: ParamEnv<'tcx>,) -> Self {
        Self {
            instance,
            body,
            tcx,
            graph: Default::default(),
            callgraph,
            worklist_instance:Default::default(),
            param_env,
        }
    }

    fn process_assignment(&mut self, place: &Place<'tcx>, rvalue: &Rvalue<'tcx>) {
        if let Some(instance_id) = self.callgraph.instance_to_index(self.instance)
        {
            let lhs_pattern = Self::process_place(place.as_ref());
            let rhs_pattern = Self::process_rvalue(rvalue);
            match (lhs_pattern, rhs_pattern) {
                
                 // a = &b
                (AccessPattern::Direct(lhs), Some(AccessPattern::Ref(rhs))) => {
                    self.graph.add_address(lhs, rhs,instance_id);
                }
                // a = b
                (AccessPattern::Direct(lhs), Some(AccessPattern::Direct(rhs))) => {
                    self.graph.add_copy(lhs, rhs,instance_id);
                }
                // a = Constant
                (AccessPattern::Direct(lhs), Some(AccessPattern::Constant(rhs))) => {
                    self.graph.add_copy_constant(lhs, rhs,instance_id);
                }
                // a = *b   //这个呢
                (AccessPattern::Direct(lhs), Some(AccessPattern::Indirect(rhs))) => {
                    self.graph.add_load(lhs, rhs,instance_id);
                }
                // *a = b
                (AccessPattern::Indirect(lhs), Some(AccessPattern::Direct(rhs))) => {
                    self.graph.add_store(lhs, rhs,instance_id);
                }
                // *a = Constant
                (AccessPattern::Indirect(lhs), Some(AccessPattern::Constant(rhs))) => {
                    self.graph.add_store_constant(lhs, rhs,instance_id);
                }
                // a = &((*b).0)  _8 = &((*_1).0: std::sync::Arc<std::sync::Mutex<i32>>);
                (AccessPattern::Direct(lhs), Some(AccessPattern::Field(rhs))) => {
                    // self.graph.add_address(lhs, rhs,instance_id);
                    println!("PROCESS FIELD");
                    println!("lhs:{:?},rhs:{:?},instance_id:{:?}",lhs,rhs,instance_id);
                    //lhs:_8; rhs:((*_1).0: std::sync::Arc<std::sync::Mutex<i32>>)
                    self.graph.add_field_address(lhs, rhs, instance_id)
                }
                _ => {}
            }
        }
    }

    fn process_place(place_ref: PlaceRef<'tcx>) -> AccessPattern<'tcx> {
        match place_ref {
            PlaceRef {
                local: l,
                projection: [ProjectionElem::Deref, ref remain @ ..],
            } => AccessPattern::Indirect(PlaceRef {
                local: l,
                projection: remain,
            }),// *a=...
            _ => AccessPattern::Direct(place_ref), // a=...
        }
    }

    fn process_rvalue(rvalue: &Rvalue<'tcx>) -> Option<AccessPattern<'tcx>> {
        match rvalue {
            Rvalue::Use(operand) | Rvalue::Repeat(operand, _) | Rvalue::Cast(_, operand, _) => {
                match operand {
                    // Operand::Move(place) | Operand::Copy(place) => {
                    //     Some(AccessPattern::Direct(place.as_ref()))
                    // }
                    //1102
                    Operand::Move(place) | Operand::Copy(place) => match place.as_ref() {
                        // _9 = (_8.0: *const alloc::sync::ArcInner<T>)
                        // PlaceRef {
                        //     local: l,
                        //     projection: [ProjectionElem::Field(_, _), ..],
                        // } => Some(AccessPattern::Field(place.as_ref())), 

                        PlaceRef {
                            local: l,
                            projection: [ProjectionElem::Deref, ref remain @ ..],
                        } => Some(AccessPattern::Indirect(PlaceRef {
                            local: l,
                            projection: remain,
                        })),// ..=*q
                        _ => Some(AccessPattern::Direct(place.as_ref())),
                    }
                    Operand::Constant(box Constant {
                        span: _,
                        user_ty: _,
                        literal,
                    }) => Some(AccessPattern::Constant(*literal)),
                }
            }

            Rvalue::Ref(_, _, place) | Rvalue::AddressOf(_, place) => match place.as_ref() {
                PlaceRef {
                    local: l,
                    projection: [..,ProjectionElem::Field(_, _),],
                } => Some(AccessPattern::Field(place.as_ref())), // ..=&((*_1).0) 1106  a = &((*b).0)

                PlaceRef {
                    local: l,
                    projection: [ProjectionElem::Deref, ref remain @ ..],
                } => Some(AccessPattern::Direct(PlaceRef {
                    local: l,
                    projection: remain,
                })), // p=&*q
                
                _ => Some(AccessPattern::Ref(place.as_ref())),
                
            },
            _ => None,
        }
    }

    /// 实参 --> 形参
    fn process_call_arg(&mut self, real: PlaceRef<'tcx>, formal: PlaceRef<'tcx>, callee_instance_id:InstanceId) {
        if let Some(instance_id) = self.callgraph.instance_to_index(self.instance)
        {
            // println!("CALL_ARG:CALLER_InstanceID{:?},real{:?},CALLEE_InstanceID{:?},formal{:?}",instance_id,real,callee_instance_id,formal);
            self.graph.add_call_arg(real, formal, instance_id, callee_instance_id)
            
        }
        
    }
    /// dest: *const T = Vec::as_ptr(arg: &Vec<T>) =>
    /// arg--|copy|-->dest
    fn process_call_arg_dest_inter(&mut self, arg: PlaceRef<'tcx>, dest: PlaceRef<'tcx>) {
        if let Some(instance_id) = self.callgraph.instance_to_index(self.instance)
        {
             self.graph.add_copy(dest, arg,instance_id);
        }
       
    }

    /// dest: Arc<T> = Arc::clone(arg: &Arc<T>) or dest: T = ptr::read(arg: *const T) =>
    /// arg--|load|-->dest and
    /// arg--|alias_copy|-->dest
    fn process_alias_copy(&mut self, arg: PlaceRef<'tcx>, dest: PlaceRef<'tcx>) {
        if let Some(instance_id) = self.callgraph.instance_to_index(self.instance)
        {
            self.graph.add_load(dest, arg,instance_id);
            self.graph.add_alias_copy(dest, arg,instance_id);
        }
        
    }

    /// forall (p1, p2) where p1 is prefix of p1, add `p1 = p2`.
    /// e.g. Place1{local1, &[f0]}, Place2{local1, &[f0,f1]},
    /// since they have the same local
    /// and Place1.projection is prefix of Place2.projection,
    /// Add constraint `Place1 = Place2`.
    fn add_partial_copy(&mut self) {
        if let Some(instance_id) = self.callgraph.instance_to_index(self.instance)
        {
            let nodes = self.graph.nodes();
            for (idx, n1) in nodes.iter().enumerate() {
                for n2 in nodes.iter().skip(idx + 1) {
                    if let (ConstraintNode::Place(p1, instance_id1), ConstraintNode::Place(p2, instance_id2)) = (n1, n2) {
                        if p1.local == p2.local {
                            if p1.projection.len() > p2.projection.len() {
                                if &p1.projection[..p2.projection.len()] == p2.projection {
                                    self.graph.add_copy(*p2,*p1,*instance_id1);
                                }
                            } else if &p2.projection[..p1.projection.len()] == p1.projection {
                                self.graph.add_copy(*p1,*p2,*instance_id2);
                            }
                        }
                    }
                }
            }
        }
    }
    

    fn analyze(
        &mut self,
        instances: Vec<&'a Instance<'tcx>>,
    ){
        //所有的instance 
       for instance_temp in instances{
            let body_temp = self.tcx.instance_mir(instance_temp.def);
            self.body = body_temp;//
            self.instance = &instance_temp;//
            self.visit_body(body_temp);
       }
    }


    fn finish(mut self) -> ConstraintGraph<'tcx> {
        self.add_partial_copy();
        // self.graph.dot();
        self.graph
    }
}

impl<'a, 'tcx> Visitor<'tcx> for ConstraintGraphCollector<'a, 'tcx> {
    fn visit_statement(&mut self, statement: &Statement<'tcx>, _location: Location) {
        match &statement.kind {
            StatementKind::Assign(box (place, rvalue)) => {
                self.process_assignment(place, rvalue);//palce左值 rvalue右值
            }
            StatementKind::FakeRead(_)
            | StatementKind::SetDiscriminant { .. }
            | StatementKind::Deinit(_)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::Retag(_, _)
            | StatementKind::AscribeUserType(_, _)
            | StatementKind::Coverage(_)
            | StatementKind::Nop
            | StatementKind::Intrinsic(_)
            | StatementKind::PlaceMention(_)
            | StatementKind::ConstEvalCounter => {}
        }
    } 

    ///处理函数调用
    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, _location: Location) {
        if let TerminatorKind::Call {
            func,//被调用的函数 Oeprand
            args,//参数 Vec<Operand>
            destination, //Place
            target,
            unwind,
            call_source,
            fn_span
        } = &terminator.kind
        {
            // let mut formal_para_vec = Vec::new();
            let func_ty = func.ty(self.body, self.tcx);
            let func_ty = self.instance.subst_mir_and_normalize_erasing_regions(
                self.tcx,
                self.param_env,
                ty::EarlyBinder::bind(func_ty),
            );
            //处理函数调用 实参和形参 
            // _10 = swap(move _11, move _14); fn swap(_1: &Mutex<i32>, _2: &Mutex<i32>);  _11 ---> _1, _14 ---> _2
            if let ty::FnDef(def_id, substs) = *func_ty.kind() {
                if let Some(callee) = Instance::resolve(self.tcx, self.param_env, def_id, substs) 
                    .ok()
                    .flatten()
                {
                    if let Some(callee_instance_id) = self.callgraph.instance_to_index(&callee){
                        if let Some(node) = self.callgraph.instance_to_CallGraphNode(&callee){
                            let formal_arg_vec = self.callgraph.getArgs(node, self.tcx);
                            let length_match = formal_arg_vec.len() == args.len();
                            if length_match {
                                let combined = args.iter().zip(formal_arg_vec.iter());
                                for (real, formal) in combined {
                                     if let Some(real) = real.place(){
                                        self.process_call_arg(real.as_ref(), formal.as_ref(), callee_instance_id);
                                     }
                                }
                            }
                            
                         }
                    }                    
                    // if let Some(callee_instance_id) = self.callgraph.instance_to_index(&callee){
                    //     if let Some(instance_id) = self.callgraph.instance_to_index(self.instance){
                    //         println!("PROCESS CALL................");
                    //         println!("InstanceId:{:?} Callee_InstanceID{:?}\n",instance_id,callee_instance_id);
                    //     }
                    // }
                }
            }  
           

           //处理dest和返回值 _4 = Mutex::<i32>::lock(_2) ， _2 ----> _4
           //   还要修改，内部函数？    这里不够准确 需要修改    
           match (args.as_slice(), destination) {
            (&[Operand::Move(arg)], dest) => {
                self.process_call_arg_dest_inter(arg.as_ref(), dest.as_ref());
            }
            (&[Operand::Move(arg), _], dest) => {
                let func_ty1 = func.ty(self.body, self.tcx);
                if let TyKind::FnDef(def_id, _) = func_ty1.kind() {
                    if ownership::is_index(*def_id, self.tcx) {
                        return self.process_call_arg_dest_inter(arg.as_ref(), dest.as_ref());
                    }
                }
            }
            (&[Operand::Copy(arg)], dest) => {
                self.process_call_arg_dest_inter(arg.as_ref(), dest.as_ref());
            }
            (&[Operand::Copy(arg), _], dest) => {
                let func_ty1 = func.ty(self.body, self.tcx);
                if let TyKind::FnDef(def_id, _) = func_ty1.kind() {
                    if ownership::is_index(*def_id, self.tcx) {
                        return self.process_call_arg_dest_inter(arg.as_ref(), dest.as_ref());
                    }
                }
            }
            _ => {}
        }
        }
    } 


    ///处理闭包 Closure 
    /// _6 = [closure@src/main.rs:43:28: 43:35] { foo1: move _3 };
    fn visit_local_decl(&mut self, local: Local, local_decl: &LocalDecl<'tcx>) {
        let func_ty = self.instance.subst_mir_and_normalize_erasing_regions(
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
                    //    println!("CLOSURE:{:?}",callee_instance);
                        //处理闭包
                    }
                }
            }
        }
    }
}