use std::cell::RefCell;

use crate::analysis::pointsto::{AliasAnalysis, ApproximateAliasKind};
use crate::concurrency::locks::{
    DeadlockPossibility, LockGuardCollector, LockGuardId, LockGuardMap, LockGuardTy,
};
use crate::graph::callgraph::{CallGraph, CallGraphNode, InstanceId};

use petgraph::dot::Dot;
use petgraph::graph::NodeIndex;
use petgraph::visit::IntoNodeReferences;
use petgraph::Graph;

use rustc_middle::mir::Location;
use rustc_middle::ty::{ParamEnv, TyCtxt};

use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Clone, Debug, Default)]
struct LiveLockGuards(FxHashSet<LockGuardId>);

impl LiveLockGuards {
    fn insert(&mut self, lockguard_id: LockGuardId) -> bool {
        self.0.insert(lockguard_id)
    }
    fn raw_lockguard_ids(&self) -> &FxHashSet<LockGuardId> {
        &self.0
    }
    // self = self \ other, if changed return true
    fn difference_in_place(&mut self, other: &Self) -> bool {
        let old_len = self.0.len();
        for id in &other.0 {
            self.0.remove(id);
        }
        old_len != self.0.len()
    }
    // self = self U other, if changed return true
    fn union_in_place(&mut self, other: Self) -> bool {
        let old_len = self.0.len();
        self.0.extend(other.0.into_iter());
        old_len != self.0.len()
    }
}

type LockGuardsBeforeCallSites = FxHashMap<(InstanceId, Location), LiveLockGuards>;

pub struct PtsDetecter<'tcx> {
    tcx: TyCtxt<'tcx>,
    param_env: ParamEnv<'tcx>,
    pub lockguard_relations: FxHashSet<(LockGuardId, LockGuardId)>,
}

impl<'tcx> PtsDetecter<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, param_env: ParamEnv<'tcx>) -> Self {
        Self {
            tcx,
            param_env,
            lockguard_relations: Default::default(),
        }
    }

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
            // Only analyze local fn with body
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

    pub fn generate_petri_net<'a>(&self, callgraph: &'a CallGraph<'tcx>) {
        let lockguards = self.collect_lockguards(callgraph);
        callgraph.dot();
    }

    pub fn output_pts<'a>(
        &mut self,
        callgraph: &'a CallGraph<'tcx>,
        alias_analysis: &mut RefCell<AliasAnalysis<'a, 'tcx>>,
    ) {
        let lockguards = self.collect_lockguards(callgraph);
        // Get lockguard info
        let mut info = FxHashMap::default();
        for (_, map) in lockguards.into_iter() {
            info.extend(map.into_iter());
        }
        for (k1, _) in info.iter() {
            for (k2, _) in info.iter() {
                self.lockguard_relations.insert((*k1, *k2));
            }
        }

        use std::rc::Rc;
        let lock_node = Rc::new(RefCell::new(FxHashMap::<LockGuardId, NodeIndex>::default()));
        let mut pts_map = Graph::<String, String>::new();
        // let record_lock = FxHashMap::<String, bool>::default();
        for (a, b) in &self.lockguard_relations {
            let possibility = deadlock_possibility(a, b, &info, alias_analysis);
            match possibility {
                DeadlockPossibility::Probably | DeadlockPossibility::Possibly => {
                    let a_info = &info[a];
                    let b_info = &info[b];
                    let a_str =
                        format!("{:?}", a_info.lockguard_ty) + &format!("{:?}", a_info.span);
                    let b_str =
                        format!("{:?}", b_info.lockguard_ty) + &format!("{:?}", b_info.span);

                    if lock_node.borrow().get(a).is_none() {
                        if lock_node.borrow().get(b).is_none() {
                            // a,b both not exit
                            let a_node = pts_map.add_node(a_str);
                            let b_node = pts_map.add_node(b_str);
                            pts_map.add_edge(a_node, b_node, "".to_string());
                            lock_node.borrow_mut().insert(*a, a_node);
                            lock_node.borrow_mut().insert(*b, b_node);
                        } else {
                            // a not exits
                            let a_node = pts_map.add_node(a_str);
                            pts_map.add_edge(
                                a_node,
                                *lock_node.borrow().get(b).unwrap(),
                                "".to_string(),
                            );
                            lock_node.borrow_mut().insert(*a, a_node);
                        }
                    } else {
                        // b not exits
                        if lock_node.borrow().get(b).is_none() {
                            let b_node = pts_map.add_node(b_str);
                            pts_map.add_edge(
                                *lock_node.borrow().get(a).unwrap(),
                                b_node,
                                "".to_string(),
                            );
                            lock_node.borrow_mut().insert(*b, b_node);
                        } else {
                            // a,b both exit
                            pts_map.add_edge(
                                *lock_node.borrow().get(a).unwrap(),
                                *lock_node.borrow().get(b).unwrap(),
                                "".to_string(),
                            );
                        }
                    }
                }
                _ => {
                    let a_info = &info[a];
                    let b_info = &info[b];
                    let a_str =
                        format!("{:?}", a_info.lockguard_ty) + &format!("{:?}", a_info.span);
                    let b_str =
                        format!("{:?}", b_info.lockguard_ty) + &format!("{:?}", b_info.span);
                    if lock_node.borrow().get(a).is_none() {
                        let a_node = pts_map.add_node(a_str);
                        lock_node.borrow_mut().insert(*a, a_node);
                    } else if lock_node.borrow().get(a).is_none() {
                        let b_node = pts_map.add_node(b_str);
                        lock_node.borrow_mut().insert(*b, b_node);
                    }
                }
            }
        }
        use std::io::Write;
        let dot = Dot::new(&pts_map);
        let mut file = std::fs::File::create("pts_map.dot").unwrap();
        write!(file, "{}", dot).unwrap();
    }
}

fn deadlock_possibility<'tcx>(
    a: &LockGuardId,
    b: &LockGuardId,
    lockguards: &LockGuardMap<'tcx>,
    alias_analysis: &mut RefCell<AliasAnalysis>,
) -> DeadlockPossibility {
    let a_ty = &lockguards[a].lockguard_ty;
    let b_ty = &lockguards[b].lockguard_ty;
    if let (LockGuardTy::ParkingLotRead(_), LockGuardTy::ParkingLotRead(_)) = (a_ty, b_ty) {
        if lockguards[b].is_gen_only_by_recursive() {
            return DeadlockPossibility::Unlikely;
        }
    }
    // Assume that a lock in a loop or recursive functions will not deadlock with itself,
    // in which case the lock spans of the two locks are the same.
    // This may miss some bugs but can reduce many FPs.
    if lockguards[a].span == lockguards[b].span {
        return DeadlockPossibility::Unlikely;
    }
    let possibility = match a_ty.deadlock_with(b_ty) {
        DeadlockPossibility::Probably => {
            match alias_analysis.borrow_mut().alias((*a).into(), (*b).into()) {
                ApproximateAliasKind::Probably => DeadlockPossibility::Probably,
                ApproximateAliasKind::Possibly => DeadlockPossibility::Possibly,
                ApproximateAliasKind::Unlikely => DeadlockPossibility::Unlikely,
                ApproximateAliasKind::Unknown => DeadlockPossibility::Unknown,
            }
        }
        DeadlockPossibility::Possibly => {
            match alias_analysis.borrow_mut().alias((*a).into(), (*b).into()) {
                ApproximateAliasKind::Probably => DeadlockPossibility::Possibly,
                ApproximateAliasKind::Possibly => DeadlockPossibility::Possibly,
                ApproximateAliasKind::Unlikely => DeadlockPossibility::Unlikely,
                ApproximateAliasKind::Unknown => DeadlockPossibility::Unknown,
            }
        }
        _ => DeadlockPossibility::Unlikely,
    };
    possibility
}
