//! 指向关系图
//! 只输出锁变量之间的指向关系，没有考虑变量的生命周期
//! 基于域敏感上下文不敏感的过程间指针分析
extern crate rustc_hash;

use crate::analysis::pointsto::AliasAnalysis;
use crate::analysis::pointsto::Andersen;
use crate::analysis::pointsto::ApproximateAliasKind;
use crate::concurrency::locks::{LockGuardCollector, LockGuardId, LockGuardMap};
use crate::graph::callgraph::{CallGraph, CallGraphNode, InstanceId};

use petgraph::dot::Dot;
use petgraph::graph::NodeIndex;
use petgraph::visit::IntoNodeReferences;
use petgraph::Graph;

use rustc_hash::{FxHashMap, FxHashSet};
use rustc_middle::ty::{ParamEnv, TyCtxt};

pub struct PtsGraph<'tcx> {
    tcx: TyCtxt<'tcx>,
    param_env: ParamEnv<'tcx>,
    pub lockguard_relations: FxHashSet<(LockGuardId, LockGuardId)>,
}

impl<'tcx> PtsGraph<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, param_env: ParamEnv<'tcx>) -> Self {
        Self {
            tcx,
            param_env,
            lockguard_relations: Default::default(),
        }
    }

    /// 收集所有锁变量
    fn collect_lockguards(
        &self,
        callgraph: &CallGraph<'tcx>,
    ) -> FxHashMap<InstanceId, LockGuardMap<'tcx>> {
        let mut lockguards = FxHashMap::default();
        for (instance_id, node) in callgraph.graph.node_references() {
            let instance = match node {
                CallGraphNode::WithBody(instance) => instance,
                _ => continue,
            };
            if !instance.def_id().is_local() {
                continue;
            }
            let body = self.tcx.instance_mir(instance.def);
            let mut lockguard_collector =
                LockGuardCollector::new(instance_id, instance, body, self.tcx, self.param_env);
            lockguard_collector.analyze();
            if !lockguard_collector.lockguards.is_empty() {
                lockguards.insert(instance_id, lockguard_collector.lockguards);
            }
        }
        lockguards
    }

    /// dot格式输出
    pub fn output_pts<'a>(
        &mut self,
        callgraph: &'a CallGraph<'tcx>,
        alias: &mut AliasAnalysis<'a, 'tcx>,
        param_env: ParamEnv<'tcx>,
    ) {
        let lockguards = self.collect_lockguards(callgraph);
        let mut info = FxHashMap::default();
        for (_, map) in lockguards.into_iter() {
            info.extend(map.into_iter());
        }
        for (k1, _) in info.iter() {
            for (k2, _) in info.iter() {
                self.lockguard_relations.insert((*k1, *k2)); //(LockGuardId,LockGuardId)
            }
        }
        use std::cell::RefCell;
        use std::rc::Rc;
        let lock_node = Rc::new(RefCell::new(FxHashMap::<LockGuardId, NodeIndex>::default()));
        let mut pts_map = Graph::<String, String>::new();
        for (a, b) in &self.lockguard_relations {
            let a_info = &info[a];
            let b_info = &info[b];
            let a_str = format!("{:?}", a_info.lockguard_ty) + &format!("{:?}", a_info.span);
            let b_str = format!("{:?}", b_info.lockguard_ty) + &format!("{:?}", b_info.span);
            let possibility = alias.alias((*a).into(), (*b).into()); //指针分析
            match possibility {
                ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                    if lock_node.borrow().get(a).is_none() {
                        if lock_node.borrow().get(b).is_none() {
                            let a_node = pts_map.add_node(a_str);
                            let b_node = pts_map.add_node(b_str);
                            pts_map.add_edge(a_node, b_node, possibility.to_string());
                            lock_node.borrow_mut().insert(*a, a_node);
                            lock_node.borrow_mut().insert(*b, b_node);
                        } else {
                            let a_node = pts_map.add_node(a_str);
                            pts_map.add_edge(
                                a_node,
                                *lock_node.borrow().get(b).unwrap(),
                                possibility.to_string(),
                            );
                            lock_node.borrow_mut().insert(*a, a_node);
                        }
                    } else {
                        if lock_node.borrow().get(b).is_none() {
                            let b_node = pts_map.add_node(b_str);
                            pts_map.add_edge(
                                *lock_node.borrow().get(a).unwrap(),
                                b_node,
                                possibility.to_string(),
                            );
                            lock_node.borrow_mut().insert(*b, b_node);
                        } else {
                            pts_map.add_edge(
                                *lock_node.borrow().get(a).unwrap(),
                                *lock_node.borrow().get(b).unwrap(),
                                possibility.to_string(),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        use std::io::Write;
        let dot = Dot::new(&pts_map);
        let mut file = std::fs::File::create("pts_graph.dot").unwrap();
        write!(file, "{}", dot).unwrap();
    }
}
