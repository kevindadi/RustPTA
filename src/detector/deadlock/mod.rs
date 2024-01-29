extern crate rustc_hash;
pub mod report;
use std::fmt::Debug;
use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;

use crate::analysis::pointsto_inter::{Andersen,ApproximateAliasKind,AliasId};
use crate::concurrency::locks::{
    DeadlockPossibility, LockGuardCollector, LockGuardId, LockGuardMap, LockGuardTy,
};
use crate::concurrency::condvar::{CondvarApi, ParkingLotCondvarApi, StdCondvarApi};

use crate::graph::callgraph::{CallGraph, CallGraphNode, InstanceId};
use super::report::{Report, ReportContent};
use report::{DeadlockDiagnosis,CondvarDeadlockDiagnosis,WaitNotifyLocks};

use petgraph::algo;
use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
use petgraph::visit::{depth_first_search, Control, DfsEvent, EdgeRef, IntoNodeReferences};
use petgraph::{Directed, Direction, Graph};

use rustc_middle::mir::{Body, Location, Operand, TerminatorKind};
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

pub struct LockAnalysis<'tcx> {
    tcx: TyCtxt<'tcx>,
    param_env: ParamEnv<'tcx>,
    pub lockguard_relations: FxHashSet<(LockGuardId, LockGuardId)>,
}

impl<'tcx> LockAnalysis<'tcx> {
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
            if !instance.def_id().is_local() {
                continue;
            }
            let body = self.tcx.instance_mir(instance.def);
            let mut lockguard_collector =
                LockGuardCollector::new(instance_id, instance, body, self.tcx, self.param_env);
            lockguard_collector.analyze();
            
            if !lockguard_collector.lockguards.is_empty() { // LockGuardMap: FxHashMap<LockGuardId, LockGuardInfo<'tcx>>;
                // println!("instance_id{:?}, lockguard_collector.lockguards{:?}",instance_id,lockguard_collector.lockguards);
                lockguards.insert(instance_id, lockguard_collector.lockguards); //<NodeIndex,LockGuardMap>
            }
        }
        
        lockguards
    }

    fn collect_condvars(&self, callgraph: &CallGraph<'tcx>) -> FxHashMap<InstanceId, CondvarApi> {
        callgraph
            .graph
            .node_references()
            .filter_map(|(instance_id, node)| {
                CondvarApi::from_instance(node.instance(), self.tcx)
                    .map(|condvar_api| (instance_id, condvar_api))
            })
            .collect()
    }

    pub fn detect<'a>(
        &mut self,
        callgraph: &'a CallGraph<'tcx>,
        andersen: &mut Andersen<'a,'tcx>,
    )->Vec<Report> {
        let lockguards = self.collect_lockguards(callgraph);
        let condvar_apis = self.collect_condvars(callgraph);
        let mut lockguards_before_condvar_apis: FxHashMap<InstanceId, LockGuardsBeforeCallSites> =
            condvar_apis
                .keys()
                .map(|instance_id| (*instance_id, FxHashMap::default()))
                .collect();
        let mut worklist = callgraph
            .graph
            .node_references()
            .map(|(instance_id, _)| instance_id)
            .collect::<VecDeque<_>>();
        let mut contexts = worklist
            .iter()
            .copied()
            .map(|id| (id, LiveLockGuards::default()))
            .collect::<FxHashMap<_, _>>(); //记录LiveLockGuard
        // 固定点算法
        while let Some(id) = worklist.pop_front() {
            if let Some(lockguard_info) = lockguards.get(&id) {
                let instance = match callgraph.index_to_instance(id).unwrap() {
                    CallGraphNode::WithBody(instance) => instance,
                    _ => continue,
                };
                let body = self.tcx.instance_mir(instance.def);
                let context = contexts[&id].clone();
                let states = self.intraproc_gen_kill(body, &context, lockguard_info);
                for edge in callgraph.graph.edges_directed(id, Direction::Outgoing) {
                    let callee = edge.target();
                    for callsite in edge.weight() {
                        let loc = match callsite.location() {
                            Some(loc) => loc,
                            None => continue,
                        };
                        let callsite_state = states[&loc].clone();
                        let changed = contexts
                            .get_mut(&callee)
                            .unwrap()
                            .union_in_place(callsite_state);
                        if changed {
                            worklist.push_back(callee);
                        }
                        if condvar_apis.contains_key(&callee) {
                            lockguards_before_condvar_apis
                                .entry(callee)
                                .or_default()
                                .entry((id, loc))
                                .or_default()
                                .union_in_place(states[&loc].clone());
                        }
                    }
                }
            } else {
                for edge in callgraph.graph.edges_directed(id, Direction::Outgoing) {
                    let callee = edge.target();
                    let context = contexts[&id].clone();
                    let changed = contexts.get_mut(&callee).unwrap().union_in_place(context);
                    if changed {
                        worklist.push_back(callee);
                    }
                    if condvar_apis.contains_key(&callee) {
                        for callsite in edge.weight() {
                            if let Some(loc) = callsite.location() {
                                lockguards_before_condvar_apis
                                    .entry(callee)
                                    .or_default()
                                    .entry((id, loc))
                                    .or_default()
                                    .union_in_place(contexts[&id].clone());
                            }
                        }
                    }
                }
            }
        }
        let mut info = FxHashMap::default();
        for (_, map) in lockguards.into_iter() {
            info.extend(map.into_iter());
        }

        let mut reports = self.detect_deadlock(callgraph,&info, andersen,&contexts);

        if !lockguards_before_condvar_apis.is_empty() {
            let condvar_repo = self.detect_condvar_misuse(
                                            &lockguards_before_condvar_apis,
                                            &condvar_apis,
                                &info,
                                            callgraph,
                                            andersen,
                                        );
            reports.extend(condvar_repo.into_iter());
        }
        reports
    }

    fn intraproc_gen_kill(
        &mut self,
        body: &'tcx Body<'tcx>,
        context: &LiveLockGuards,
        lockguard_info: &LockGuardMap<'tcx>,
    ) -> FxHashMap<Location, LiveLockGuards> {
        let (gen_map, kill_map) = Self::gen_kill_locations(lockguard_info);
        let mut worklist: VecDeque<Location> = Default::default();
        for (bb, bb_data) in body.basic_blocks.iter_enumerated() {
            for stmt_idx in 0..bb_data.statements.len() + 1 {
                worklist.push_back(Location {
                    block: bb,
                    statement_index: stmt_idx,
                });
            }
        }
        let mut states: FxHashMap<Location, LiveLockGuards> = worklist
            .iter()
            .copied()
            .map(|loc| (loc, LiveLockGuards::default()))
            .collect();
        *states.get_mut(&Location::START).unwrap() = context.clone();
        while let Some(loc) = worklist.pop_front() {
            let mut after = states[&loc].clone();
            let relation = Self::apply_gen_kill(&mut after, gen_map.get(&loc), kill_map.get(&loc));
            self.lockguard_relations.extend(relation.into_iter());
            let term_loc = body.terminator_loc(loc.block);
            if loc != term_loc {
                // if not terminator
                let succ = loc.successor_within_block();
                // check lockguard relations
                // union and reprocess if changed
                let changed = states.get_mut(&succ).unwrap().union_in_place(after);
                if changed {
                    worklist.push_back(succ);
                }
            } else {
                // if is terminator
                for succ_bb in body[loc.block].terminator().successors() {
                    let succ = Location {
                        block: succ_bb,
                        statement_index: 0,
                    };
                    // union and reprocess if changed
                    let changed = states.get_mut(&succ).unwrap().union_in_place(after.clone());
                    if changed {
                        worklist.push_back(succ);
                    }
                }
            }
        }
        states
    }
    
    fn gen_kill_locations(
        lockguard_map: &LockGuardMap<'tcx>,
    ) -> (
        FxHashMap<Location, LiveLockGuards>,
        FxHashMap<Location, LiveLockGuards>,
    ) {
        let mut gen_map: FxHashMap<Location, LiveLockGuards> = Default::default();
        let mut kill_map: FxHashMap<Location, LiveLockGuards> = Default::default();
        for (id, info) in lockguard_map {
            for loc in &info.gen_locs {
                gen_map.entry(*loc).or_default().insert(*id);
            }
            for loc in &info.kill_locs {
                kill_map.entry(*loc).or_default().insert(*id);
            }
        }
        (gen_map, kill_map)
    }

    /// state' = state \ kill U gen
    /// return lockguard relation(a, b) where a is still live when b becomes live.
    fn apply_gen_kill(
        state: &mut LiveLockGuards,
        gen: Option<&LiveLockGuards>,
        kill: Option<&LiveLockGuards>,
    ) -> FxHashSet<(LockGuardId, LockGuardId)> {
        // First kill, then gen
        if let Some(kill) = kill {
            state.difference_in_place(kill);
        }
        let mut relations = FxHashSet::default();
        if let Some(gen) = gen {
            for s in state.raw_lockguard_ids() {
                for g in gen.raw_lockguard_ids() {
                    relations.insert((*s, *g));
                }
            }
            state.union_in_place(gen.clone());
        }
        relations
    }

    fn detect_deadlock<'a>(
        &self,
        callgraph: &'a CallGraph<'tcx>,
        info: &LockGuardMap<'tcx>,
        andersen: &mut Andersen<'a, 'tcx>,
        context: &FxHashMap<NodeIndex,LiveLockGuards>,
    )  -> Vec<Report>{
       let mut reports = Vec::new();
       let mut conflictlock_graph = ConflictLockGraph::new();
       let mut relation_to_nodes = FxHashMap::default();
       //处理double lock
       for (a, b) in &self.lockguard_relations { //lockguard_relations 
           let a_info = &info[a];
           let b_info = &info[b];
           let a_str =
               format!("{:?}", a_info.lockguard_ty) + &format!("{:?}", a_info.span);
           let b_str =
               format!("{:?}", b_info.lockguard_ty) + &format!("{:?}", b_info.span);
        //    println!("\nLOCKGUARD RELATION (a,b):\na: {:?},{:?}\nb: {:?},{:?}",a,a_str,b,b_str);
          
           let (possibility,reason) =deadlock_possibility(a, b,info,andersen,self.param_env); //alias
          
           match possibility {
            DeadlockPossibility::Probably | DeadlockPossibility::Possibly => {
                let diagnosis = diagnose_doublelock(a, b, info, callgraph, self.tcx);
                let report = Report::DoubleLock(ReportContent::new(
                    "DoubleLock".to_owned(),
                    format!("{:?}", possibility),
                    diagnosis,
                    "The first lock is not released when acquiring the second lock".to_owned(),
                ));
                reports.push(report);
            }
            _ if NotDeadlockReason::RecursiveRead != reason
                && NotDeadlockReason::SameSpan != reason =>
            {
                if !info[a].is_gen_only_by_move() && !info[b].is_gen_only_by_move()
                {
                    let node = conflictlock_graph.add_node((*a, *b));
                    relation_to_nodes.insert((*a, *b), node);
                }
            }
            _ => {}
           }
       }
       //处理 conflict lock
       for ((_, a), node1) in relation_to_nodes.iter() {
            for ((b, _), node2) in relation_to_nodes.iter() { //不会同时匹配同一个元素
                    let (possibility, _) = deadlock_possibility(a, b, info,andersen,self.param_env);
                    match possibility {           
                        DeadlockPossibility::Probably | DeadlockPossibility::Possibly => {
                            conflictlock_graph.add_edge(*node1, *node2, possibility); //添加边
                        }
                        _ => {}
                    };
                
               
            }
        }
        let cycle_paths = conflictlock_graph.cycle_paths();
       
        

        for path in cycle_paths {

            // let gatelock = path
            //         .iter()
            //         .zip(path.iter().skip(1).chain(path.get(0)))
            //         .map(|(node1, node2)| self.detect_gatelock(*node1, *node2, context, andersen)) 
            //         .collect::<Vec<_>>();
            // println!("gatelock{:?}",gatelock);
           
            // if !gatelock[0]{
                
                let diagnosis = path
                .into_iter()
                .map(|relation_id| { 
                    let (a, b) = conflictlock_graph.node_weight(relation_id).unwrap();
                    diagnose_one_relation(a, b, info, callgraph, self.tcx)
                })
                .collect::<Vec<_>>();
                let report = Report::ConflictLock(ReportContent::new(
                    "ConflictLock".to_owned(),
                    "Possibly".to_owned(),
                    diagnosis,
                    "Locks mutually wait for each other to form a cycle".to_owned(),
                ));
                reports.push(report);
            // }
        }
        reports
    }

    fn detect_gatelock<'a>(
        &self,
        node1:NodeIndex,
        node2:NodeIndex,
        contexts: &FxHashMap<NodeIndex,LiveLockGuards>,
        andersen: &mut Andersen<'a, 'tcx>,
    )-> bool{
        let mut live1 =  contexts[&node1].clone();
        let mut live2 =  contexts[&node2].clone();
        for id1 in live1.0.iter(){
            for id2 in live2.0.iter(){
                if *id1 == *id2{
                    return true
                }else {
                    let res = andersen.alias((*id1).into(), (*id2).into(), self.param_env);
                    match res {
                        ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably => {return true;}
                        _ =>{}
                    }
                }   
            }
        }
        false
    }
    fn detect_condvar_misuse<'a>(
        &self,
        lockguards_before_condvar_apis: &FxHashMap<InstanceId, LockGuardsBeforeCallSites>,
        condvar_apis: &FxHashMap<InstanceId, CondvarApi>,
        lockguards: &LockGuardMap<'tcx>,
        callgraph: &'a CallGraph<'tcx>,
        andersen: &mut Andersen<'a, 'tcx>,
    ) -> Vec<Report> {
        let mut reports = Vec::new();
        // Collect Condvar API info
        // callee_instance -> (Caller, Location, Callee) -> (&Condvar, MutexGuard)
        let mut std_notify = FxHashMap::default();
        // callee_instance -> (Caller, Location, Callee) -> &Condvar
        let mut std_wait = FxHashMap::default();
        // callee_instance -> (Caller, Location, Callee) -> (&Condvar, &mut MutexGuard)
        let mut parking_lot_notify = FxHashMap::default();
        // callee_instance -> (Caller, Location, Callee) -> &Condvar
        let mut parking_lot_wait = FxHashMap::default();
        for (callee_id, callsite_lockguards) in lockguards_before_condvar_apis {
            // println!("callsite_lockguards:{:?}",callsite_lockguards);
            let condvar_api = condvar_apis.get(callee_id).unwrap();
            for (caller_id, loc) in callsite_lockguards.keys() {
                let body = self.tcx.instance_mir(
                    callgraph
                        .index_to_instance(*caller_id)
                        .unwrap()
                        .instance()
                        .def,
                );
                let term = body[loc.block].terminator();
                let args = match &term.kind {
                    TerminatorKind::Call { func: _, args, .. } => args.clone(),
                    _ => continue,
                };
                // println!("condvar_api{:?},args{:?}",condvar_api,args);   //args的类型 类型
                match condvar_api {
                    CondvarApi::Std(StdCondvarApi::Wait(_)) => {
                        if let Operand::Copy(condvar_ref)|Operand::Move(condvar_ref) = &args[0]{ //
                            if let Operand::Copy(mutex_guard)|Operand::Move(mutex_guard) = &args[1]{ //
                                std_wait.insert(
                                    (*caller_id, *loc, *callee_id),
                                    (
                                        AliasId {
                                            instance_id: *caller_id,
                                            local: condvar_ref.local,
                                        },
                                        AliasId {
                                            instance_id: *caller_id,
                                            local: mutex_guard.local,
                                        },
                                    ),
                                );
                            }
                        }
                       
                    }
                    CondvarApi::ParkingLot(ParkingLotCondvarApi::Wait(_)) => {
                        if let Operand::Copy(condvar_ref)|Operand::Move(condvar_ref) = &args[0]{ //
                            if let Operand::Copy(mutex_guard_ref)|Operand::Move(mutex_guard_ref) = &args[1]{ //
                                parking_lot_wait.insert(
                                    (*caller_id, *loc, *callee_id),
                                    (
                                        AliasId {
                                            instance_id: *caller_id,
                                            local: condvar_ref.local,
                                        },
                                        AliasId {
                                            instance_id: *caller_id,
                                            local: mutex_guard_ref.local,
                                        },
                                    ),
                                );
                            }
                        }
                       
                    }
                    CondvarApi::Std(StdCondvarApi::Notify(_)) => {
                        if let Operand::Copy(condvar_ref) | Operand::Move(condvar_ref) = args[0] {
                            // callsite -> &Condvar
                            std_notify.insert(
                                (*caller_id, *loc, *callee_id),
                                AliasId {
                                    instance_id: *caller_id,
                                    local: condvar_ref.local,
                                },
                            );
                        }
                    }
                    CondvarApi::ParkingLot(ParkingLotCondvarApi::Notify(_)) => {
                        if let Operand::Copy(condvar_ref)|Operand::Move(condvar_ref) = args[0] {
                            // callsite -> &Condvar
                            parking_lot_notify.insert(
                                (*caller_id, *loc, *callee_id),
                                AliasId {
                                    instance_id: *caller_id,
                                    local: condvar_ref.local,
                                },
                            );
                        }
                    }
                }
            }
        }
       
        
        for ((caller_id1, loc1, callee_id1), (condvar_ref1, mutex_guard1)) in std_wait.iter() {
            let mut wait_match_notify = false;
            for ((caller_id2, loc2, callee_id2), condvar_ref2) in std_notify.iter() {
                let res = andersen.alias(*condvar_ref1, *condvar_ref2,self.param_env);
                // println!("condvar1:{:?},condvar2:{:?},alias:{:?}",*condvar_ref1,*condvar_ref2,res);
                match res {
                    ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably => {
                        wait_match_notify = true;
                        // live1: LiveLockGuards before `wait`
                        // live2: LiveLockGuards before `notify`
                        let live1 = lockguards_before_condvar_apis
                            .get(callee_id1)
                            .and_then(|cs| cs.get(&(*caller_id1, *loc1)));
                        let live2 = lockguards_before_condvar_apis
                            .get(callee_id2)
                            .and_then(|cs| cs.get(&(*caller_id2, *loc2)));
                        match (live1, live2) {
                            (Some(live1), Some(live2)) => {
                                // aliased_pairs = {(l1, l2) | (l1, l2) in live1 X live2 and alias(l1, l2)}
                                let live1 = live1.raw_lockguard_ids().iter();
                                let live2 = live2.raw_lockguard_ids().iter();
                                let cartesian_product =
                                    live2.flat_map(|g2| live1.clone().map(move |g1| (*g1, *g2)));
                                let aliased_pairs = cartesian_product
                                    .filter(|(g1, g2)| {
                                        andersen.alias((*g1).into(), (*g2).into(),self.param_env) //wait和notify配对 但因为闭包参数不准确
                                            > ApproximateAliasKind::Unlikely
                                            && deadlock_possibility(
                                                g1,
                                                g2,
                                                lockguards,
                                                andersen,
                                                self.param_env
                                            )
                                            .0 > DeadlockPossibility::Unlikely
                                    })
                                    .collect::<Vec<_>>();
                                // exists (g1, g2) in aliased_pairs: alias(g2, mutex_guard1)
                                // LockGuard pairs that do not alias with MutexGuard in `wait`
                                let mut no_mutex_guards = Vec::new();
                                for (g1, g2) in aliased_pairs.iter() {
                                    if AliasId::from(*g1) != *mutex_guard1 {
                                        no_mutex_guards.push((g1, g2));
                                    }
                                }
                                if !no_mutex_guards.is_empty() {
                                
                                    let diagnosis = diagnose_condvar_deadlock(
                                        (*caller_id1, *loc1),
                                        (*caller_id2, *loc2),
                                        true,
                                        &no_mutex_guards,
                                        lockguards,
                                        callgraph,
                                        self.tcx,
                                    );
                                    let content = ReportContent::new(
                                        "Deadlock before Condvar::wait and notify".to_owned(),
                                        "Possibly".to_owned(),
                                        diagnosis,
                                        "The same lock before Condvar::wait and notify".to_owned(),
                                    );
                                    let report = Report::CondvarDeadlock(content);
                                    reports.push(report);
                                }
                            }
                            (Some(_), None) => {}
                            _ => {
                                // There must be a MutexGuard before `wait`.
                                unreachable!()
                            }
                        }
                    }
                    _ => {
                    }
                }
            }
            if !wait_match_notify{
                ///需要输出report 
                /// 因为闭包参数不准确
                println!("Miss Notify()!");
                
            }
        }
        // Check parking_lot::Condvar
        for ((caller_id1, loc1, callee_id1), (condvar_ref1, mutex_guard1)) in parking_lot_wait.iter(){
            for ((caller_id2, loc2, callee_id2), condvar_ref2) in parking_lot_notify.iter() {
                let res = andersen.alias(*condvar_ref1, *condvar_ref2,self.param_env);
                match res {
                    ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably => {
                        // live1: LiveLockGuards before `wait`
                        // live2: LiveLockGuards before `notify`
                        let live1 = lockguards_before_condvar_apis
                            .get(callee_id1)
                            .and_then(|cs| cs.get(&(*caller_id1, *loc1)));
                        let live2 = lockguards_before_condvar_apis
                            .get(callee_id2)
                            .and_then(|cs| cs.get(&(*caller_id2, *loc2)));
                        match (live1, live2) {
                            (Some(live1), Some(live2)) => {
                                // aliased_pairs = {(l1, l2) | (l1, l2) in live1 X live2 and alias(l1, l2)}
                                let live1 = live1.raw_lockguard_ids().iter();
                                let live2 = live2.raw_lockguard_ids().iter();
                                let cartesian_product =
                                    live2.flat_map(|g2| live1.clone().map(move |g1| (*g1, *g2)));
                                let aliased_pairs = cartesian_product
                                    .filter(|(g1, g2)| {
                                       andersen.alias((*g1).into(), (*g2).into(),self.param_env)
                                            > ApproximateAliasKind::Unlikely
                                            && deadlock_possibility(
                                                g1,
                                                g2,
                                                lockguards,
                                                andersen,
                                                self.param_env
                                            )
                                            .0 > DeadlockPossibility::Unlikely
                                    })
                                    .collect::<Vec<_>>();
                                // exists (g1, g2) in aliased_pairs: alias(g2, mutex_guard1)
                                // LockGuard pairs that do not alias with MutexGuard in `wait`
                                let mut no_mutex_guards = Vec::new();
                                for (g1, g2) in aliased_pairs.iter() {
                                    if !matches!(
                                        andersen.points(*mutex_guard1, AliasId::from(*g1),self.param_env), //
                                        ApproximateAliasKind::Possibly
                                            | ApproximateAliasKind::Probably
                                    ) {
                                        no_mutex_guards.push((g1, g2));
                                    }
                                }
                                if !no_mutex_guards.is_empty() {
                                    let diagnosis = diagnose_condvar_deadlock(
                                        (*caller_id1, *loc1),
                                        (*caller_id2, *loc2),
                                        false,
                                        &no_mutex_guards,
                                        lockguards,
                                        callgraph,
                                        self.tcx,
                                    );
                                    let content = ReportContent::new(
                                        "Deadlock before Condvar::wait and notify".to_owned(),
                                        "Possibly".to_owned(),
                                        diagnosis,
                                        "The same lock before Condvar::wait and notify".to_owned(),
                                    );
                                    let report = Report::CondvarDeadlock(content);
                                    reports.push(report);
                                }
                            }
                            (Some(_), None) => {}
                            _ => {
                                // There must be a MutexGuard before `wait`.
                                unreachable!()
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        
        reports
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotDeadlockReason {
    TrueDeadlock,
    RecursiveRead,
    SameSpan,
    // TODO,
}

///先判断类型,再进行alias;如果类型不同,就不会有双锁错误
fn deadlock_possibility<'a,'tcx>(
    a: &LockGuardId,
    b: &LockGuardId,
    lockguards: &LockGuardMap<'tcx>,
    andersen: &mut Andersen<'a,'tcx>,
    param_env: ParamEnv<'tcx>,
) -> (DeadlockPossibility, NotDeadlockReason) {
    
    let a_ty = &lockguards[a].lockguard_ty;
    let b_ty = &lockguards[b].lockguard_ty;
    if let (LockGuardTy::ParkingLotRead(_), LockGuardTy::ParkingLotRead(_)) = (a_ty, b_ty) {
        if lockguards[b].is_gen_only_by_recursive() {
            return (
                DeadlockPossibility::Unlikely,
                NotDeadlockReason::RecursiveRead,
            );
        }
    }
    // Assume that a lock in a loop or recursive functions will not deadlock with itself,
    // in which case the lock spans of the two locks are the same.
    // This may miss some bugs but can reduce many FPs.
    // 循环或者递归内的锁 假设不会造成死锁??
    if lockguards[a].span == lockguards[b].span {
        return (DeadlockPossibility::Unlikely, NotDeadlockReason::SameSpan);
    }
    let possibility = match a_ty.deadlock_with(b_ty) {
        DeadlockPossibility::Probably => match andersen.alias((*a).into(), (*b).into(),param_env) { //
            ApproximateAliasKind::Probably => DeadlockPossibility::Probably,
            ApproximateAliasKind::Possibly => DeadlockPossibility::Possibly,
            ApproximateAliasKind::Unlikely => DeadlockPossibility::Unlikely,
            ApproximateAliasKind::Unknown => DeadlockPossibility::Unknown,
        },
        DeadlockPossibility::Possibly => match andersen.alias((*a).into(), (*b).into(),param_env) {
            ApproximateAliasKind::Probably => DeadlockPossibility::Possibly,
            ApproximateAliasKind::Possibly => DeadlockPossibility::Possibly,
            ApproximateAliasKind::Unlikely => DeadlockPossibility::Unlikely,
            ApproximateAliasKind::Unknown => DeadlockPossibility::Unknown,
        },
        _ => DeadlockPossibility::Unlikely,
    };
    (possibility, NotDeadlockReason::TrueDeadlock)
}

fn diagnose_doublelock<'tcx>(
    a: &LockGuardId,
    b: &LockGuardId,
    lockguards: &LockGuardMap<'tcx>,
    callgraph: &CallGraph<'tcx>,
    tcx: TyCtxt<'tcx>,
) -> DeadlockDiagnosis {
    diagnose_one_relation(a, b, lockguards, callgraph, tcx)
}
fn diagnose_condvar_deadlock<'tcx>(
    callsite1: (InstanceId, Location),
    callsite2: (InstanceId, Location),
    is_std_condvar: bool,
    aliased_pairs: &[(&LockGuardId, &LockGuardId)],
    lockguards: &LockGuardMap<'tcx>,
    callgraph: &CallGraph<'tcx>,
    tcx: TyCtxt<'tcx>,
) -> CondvarDeadlockDiagnosis {
    let (caller_id1, loc1) = callsite1;
    let (caller_id2, loc2) = callsite2;
    let caller_body1 = tcx.instance_mir(
        callgraph
            .index_to_instance(caller_id1)
            .unwrap()
            .instance()
            .def,
    );
    let caller_body2 = tcx.instance_mir(
        callgraph
            .index_to_instance(caller_id2)
            .unwrap()
            .instance()
            .def,
    );
    let wait_span = format!("{:?}", caller_body1.source_info(loc1).span);
    let notify_span = format!("{:?}", caller_body2.source_info(loc2).span);
    let wait_notify_locks = aliased_pairs
        .iter()
        .map(|(a, b)| {
            let a_info = &lockguards[a];
            let b_info = &lockguards[b];
            WaitNotifyLocks::new(
                format!("{:?}", a_info.lockguard_ty),
                format!("{:?}", a_info.span),
                format!("{:?}", b_info.lockguard_ty),
                format!("{:?}", b_info.span),
            )
        })
        .collect::<Vec<_>>();
    if is_std_condvar {
        CondvarDeadlockDiagnosis::new(
            "std::sync::Condvar::wait".to_owned(),
            wait_span,
            "std::sync::Condvar::notify".to_owned(),
            notify_span,
            wait_notify_locks,
        )
    } else {
        CondvarDeadlockDiagnosis::new(
            "parking_lot::Condvar::wait".to_owned(),
            wait_span,
            "parking_lot::Condvar::notify".to_owned(),
            notify_span,
            wait_notify_locks,
        )
    }
}

// fn diagnose_condvar_no_notify<'tcx>(
//     callsite1: (InstanceId, Location),
//     is_std_condvar: bool,
//     waitid: LockGuardId,
//     lockguards: &LockGuardMap<'tcx>,
//     callgraph: &CallGraph<'tcx>,
//     tcx: TyCtxt<'tcx>,
// ) -> CondvarDeadlockDiagnosis{
//     CondvarDeadlockDiagnosis::new(
//         "std::sync::Condvar::wait".to_owned(),
//         wait_span,
//         "std::sync::Condvar::notify".to_owned(),
//         notify_span,
//         wait_notify_locks,
//     )
// }




/// Find all the callchains: source -> target
// e.g., for one path: source --|callsites1|--> medium --|callsites2|--> target,
// first extract callsite locations on edge, namely, [callsites1, callsites2],
// then map locations to spans [spans1, spans2].
fn track_callchains<'tcx>(
    source: InstanceId,
    target: InstanceId,
    callgraph: &CallGraph<'tcx>,
    tcx: TyCtxt<'tcx>,
) -> Vec<Vec<Vec<String>>> {
    let paths = callgraph.all_simple_paths(source, target);
    paths
        .into_iter()
        .map(|vec| {
            vec.windows(2)
                .map(|window| {
                    let (caller, callee) = (window[0], window[1]);
                    let caller_instance = match callgraph.index_to_instance(caller).unwrap() {
                        CallGraphNode::WithBody(instance) => instance,
                        n => panic!("CallGraphNode {:?} must own body", n),
                    };
                    let caller_body = tcx.instance_mir(caller_instance.def);
                    let callsites = callgraph.callsites(caller, callee).unwrap();
                    callsites
                        .into_iter()
                        .filter_map(|location| {
                            location
                                .location()
                                .map(|loc| format!("{:?}", caller_body.source_info(loc).span))
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
}

// Find the diagnosis info for relation(a, b), including a's ty & span, b's ty & span, and callchains.
fn diagnose_one_relation<'tcx>(
    a: &LockGuardId,
    b: &LockGuardId,
    lockguards: &LockGuardMap<'tcx>,
    callgraph: &CallGraph<'tcx>,
    tcx: TyCtxt<'tcx>,
) -> DeadlockDiagnosis {
    let a_info = &lockguards[a];
    let b_info = &lockguards[b];
    let first_lock = (
        format!("{:?}", a_info.lockguard_ty),
        format!("{:?}", a_info.span),
    );
    let second_lock = (
        format!("{:?}", b_info.lockguard_ty),
        format!("{:?}", b_info.span),
    );
    let callchains = track_callchains(a.instance_id, b.instance_id, callgraph, tcx);
    DeadlockDiagnosis::new(
        first_lock.0,
        first_lock.1,
        second_lock.0,
        second_lock.1,
        callchains,
    )
}

type RelationId = NodeIndex;
struct ConflictLockGraph {
    graph: Graph<(LockGuardId, LockGuardId), DeadlockPossibility, Directed>,
}

impl ConflictLockGraph {
    fn new() -> Self {
        Self {
            graph: Graph::new(),
        }
    }
    fn add_node(&mut self, relation: (LockGuardId, LockGuardId)) -> RelationId {
        self.graph.add_node(relation)
    }

    fn add_edge(&mut self, a: RelationId, b: RelationId, weight: DeadlockPossibility) {
        self.graph.add_edge(a, b, weight);
    }

    fn node_weight(&self, a: RelationId) -> Option<&(LockGuardId, LockGuardId)> {
        self.graph.node_weight(a)
    }

    /// Find all the back-edges in the graph.
    fn back_edges(&self) -> Vec<(RelationId, RelationId)> {
        let mut back_edges = Vec::new();
        let nodes = self.graph.node_indices();
        for start in nodes {
            depth_first_search(&self.graph, Some(start), |event| {
                match event {
                    DfsEvent::BackEdge(u, v) => {
                        if !back_edges.contains(&(u, v)) && !back_edges.contains(&(v, u)) {
                            back_edges.push((u, v));
                        }
                    }
                    DfsEvent::Finish(_, _) => {
                        return Control::Break(());
                    }
                    _ => {}
                };
                Control::Continue
            });
        }
        back_edges
    }

    /// Find all the cycles in the graph.
    fn cycle_paths(&self) -> Vec<Vec<RelationId>> {
        let mut dedup = Vec::new();
        let mut edge_sets = Vec::new();
        for (src, target) in self.back_edges() {
            let cycle_paths =
                algo::all_simple_paths::<Vec<_>, _>(&self.graph, target, src, 0, None)
                    .collect::<Vec<_>>();
            for path in cycle_paths {
                // `path` forms a cycle, where adjacent nodes are directly connected and last_node connects to first_node.
                // Different `path`s beginning with different nodes may denote the same cycle if their edges are the same.
                // Thus we use `edge_sets` to deduplicate the cycle paths.
                let set = path
                    .iter()
                    .zip(path.iter().skip(1).chain(path.get(0)))
                    .map(|(a, b)| (*a, *b))
                    .collect::<FxHashSet<_>>();
                if !edge_sets.contains(&set) {
                    edge_sets.push(set);
                    dedup.push(path);
                }
            }
        }
        dedup
    }

    /// Print the ConflictGraph in dot format.
    #[allow(dead_code)]
    fn dot(&self) {
        // println!(
        //     "{:?}",
        //     Dot::with_config(&self.graph, &[Config::GraphContentOnly])
        // );
        let dot_string = format!("digraph G {{\n{:?}\n}}", Dot::with_config(&self.graph, &[Config::GraphContentOnly]));
        let output_file_path = "conflictGraph.dot";
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