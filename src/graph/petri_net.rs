extern crate rustc_hash;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use log::debug;
use log::info;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::visit::IntoNodeReferences;
use petgraph::Direction;
use petgraph::Graph;
use rustc_hash::{FxHashMap, FxHashSet};
use rustc_hir::def_id::DefId;
use rustc_middle::{
    mir::{
        visit::{MutatingUseContext, NonMutatingUseContext, PlaceContext, Visitor},
        Body, Local, Location, Terminator, TerminatorKind,
    },
    ty::{self, Instance, ParamEnv, TyCtxt},
};

use super::callgraph::{CallGraph, CallGraphNode, InstanceId};
use super::function_pn::FunctionPN;
use super::state_graph::{StateGraph, StateNode};
use crate::{
    analysis::pointsto::{AliasAnalysis, ApproximateAliasKind},
    concurrency::locks::{
        DeadlockPossibility, LockGuardCollector, LockGuardId, LockGuardMap, LockGuardTy,
    },
};

#[derive(Debug, Clone)]
pub enum Shape {
    Circle,
    Box,
}

#[derive(Debug, Clone)]
pub struct Place {
    name: String,
    tokens: RefCell<u32>,
    capacity: u32,
    shape: Shape,
    terminal_mark: bool,
}

impl Place {
    pub fn new(name: String, token: u32) -> Self {
        Self {
            name,
            tokens: RefCell::new(token),
            capacity: 1u32,
            shape: Shape::Circle,
            terminal_mark: false,
        }
    }

    pub fn new_with_no_token(name: String) -> Self {
        Self {
            name,
            tokens: RefCell::new(0u32),
            capacity: 1u32,
            shape: Shape::Circle,
            terminal_mark: false,
        }
    }

    pub fn new_with_terminal_mark(name: String, token: u32, terminal_mark: bool) -> Self {
        Self {
            name,
            tokens: RefCell::new(token),
            capacity: 1u32,
            shape: Shape::Circle,
            terminal_mark,
        }
    }
}

impl std::fmt::Display for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug, Clone)]
pub struct Transition {
    name: String,
    time: (u32, u32),
    weight: u32,
    shape: Shape,
}

impl Transition {
    pub fn new(name: String, time: (u32, u32), weight: u32) -> Self {
        Self {
            name,
            time,
            weight,
            shape: Shape::Box,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PetriNetNode {
    P(Place),
    T(Transition),
}

impl std::fmt::Display for PetriNetNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PetriNetNode::P(place) => write!(f, "{}", place.name),
            PetriNetNode::T(transition) => write!(f, "{}", transition.name),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PetriNetEdge {
    pub label: u32,
}

impl std::fmt::Display for PetriNetEdge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

pub struct PetriNet<'a, 'tcx> {
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    param_env: ParamEnv<'tcx>,
    pub net: Graph<PetriNetNode, PetriNetEdge>,
    callgraph: &'a CallGraph<'tcx>,
    function_counter: HashMap<DefId, (NodeIndex, NodeIndex, NodeIndex, NodeIndex)>,
    pub function_vec: HashMap<DefId, Vec<NodeIndex>>,
    locks_counter: HashMap<LockGuardId, NodeIndex>,
    lock_info: LockGuardMap<'tcx>,
    deadlock_marks: HashSet<Vec<usize>>,
    // threads: VecDeque<Rc<Thread>>,
}

// impl std::fmt::Display for PetriNet {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         let config = Config::default()
//             .node_shape(|node, _| match &self.net[node] {
//                 PetriNetNode::P(_) => "circle",
//                 PetriNetNode::T(_) => "rectangle",
//             })
//             .node_style(|_, _| "filled")
//             .edge_style(|_, _| "solid");

//         write!(f, "{}", Dot::with_config(&self.net, &[config]))
//     }
// }

impl<'a, 'tcx> PetriNet<'a, 'tcx> {
    pub fn new(
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
        param_env: ParamEnv<'tcx>,
        callgraph: &'a CallGraph<'tcx>,
    ) -> Self {
        Self {
            tcx,
            param_env,
            net: Graph::<PetriNetNode, PetriNetEdge>::new(),
            callgraph,
            function_counter: HashMap::<DefId, (NodeIndex, NodeIndex, NodeIndex, NodeIndex)>::new(),
            function_vec: HashMap::<DefId, Vec<NodeIndex>>::new(),
            locks_counter: HashMap::<LockGuardId, NodeIndex>::new(),
            lock_info: HashMap::default(),
            deadlock_marks: HashSet::<Vec<usize>>::new(),
        }
    }

    pub fn construct(&mut self, alias_analysis: &mut AliasAnalysis) {
        self.construct_func();
        self.construct_lock_with_dfs(alias_analysis);
        for (node, caller) in self.callgraph.graph.node_references() {
            let body = self.tcx.instance_mir(caller.instance().def);
            // Skip promoted src
            if body.source.promoted.is_some() {
                continue;
            }
            let lock_infos = self.lock_info.clone();
            // let mut link_construct = LinkConstruct::new(
            //     node,
            //     caller.instance(),
            //     body,
            //     self.tcx,
            //     self.param_env,
            //     &mut self.net,
            //     lock_infos.clone(),
            //     &self.function_counter,
            //     &mut self.function_vec,
            //     &self.locks_counter,
            // );
            // link_construct.visit_body(body);

            let mut func_construct = FunctionPN::new(
                node,
                caller.instance(),
                body,
                self.tcx,
                self.param_env,
                &mut self.net,
                lock_infos,
                &self.function_counter,
                &self.locks_counter,
            );
            func_construct.visit_body(body);
        }

        //self.deal_post_function();
    }

    // Construct Function Start and End Place by callgraph
    pub fn construct_func(&mut self) {
        if let Some((main_func, _)) = self.tcx.entry_fn(()) {
            for node_idx in self.callgraph.graph.node_indices() {
                // println!("{:?}", self.callgraph.graph.node_weight(node_idx).unwrap());
                let func_instance = self.callgraph.graph.node_weight(node_idx).unwrap();
                let func_id = func_instance.instance().def_id();
                let func_name = self.tcx.def_path_str(func_id);
                if func_name.contains("core")
                    || func_name.contains("std")
                    || func_name.contains("alloc")
                    || func_name.contains("parking_lot::")
                    || func_name.contains("spin::")
                    || func_name.contains("::new")
                {
                    continue;
                }
                if func_id == main_func {
                    let func_start = Place::new(format!("{}", func_name) + "start", 1);
                    let func_start_node_id = self.net.add_node(PetriNetNode::P(func_start));
                    let func_end = Place::new_with_no_token(format!("{}", func_name) + "end");
                    let func_end_node_id = self.net.add_node(PetriNetNode::P(func_end));
                    let func_panic = Place::new_with_no_token(format!("{}", func_name) + "panic");
                    let func_panic_node_id = self.net.add_node(PetriNetNode::P(func_panic));
                    let func_unwind = Place::new_with_no_token(format!("{}", func_name) + "unwind");
                    let func_unwind_node_id = self.net.add_node(PetriNetNode::P(func_unwind));
                    self.function_counter.insert(
                        func_id,
                        (
                            func_start_node_id,
                            func_end_node_id,
                            func_panic_node_id,
                            func_unwind_node_id,
                        ),
                    );
                    // self.function_vec.push(func_start_node_id);
                    self.function_vec.insert(func_id, vec![func_start_node_id]);
                } else {
                    let func_start = Place::new_with_no_token(format!("{}", func_name) + "start");
                    let func_start_node_id = self.net.add_node(PetriNetNode::P(func_start));
                    let func_end = Place::new_with_no_token(format!("{}", func_name) + "end");
                    let func_end_node_id = self.net.add_node(PetriNetNode::P(func_end));
                    let func_panic = Place::new_with_no_token(format!("{}", func_name) + "panic");
                    let func_panic_node_id = self.net.add_node(PetriNetNode::P(func_panic));
                    let func_unwind = Place::new_with_no_token(format!("{}", func_name) + "unwind");
                    let func_unwind_node_id = self.net.add_node(PetriNetNode::P(func_unwind));
                    self.function_counter.insert(
                        func_id,
                        (
                            func_start_node_id,
                            func_end_node_id,
                            func_panic_node_id,
                            func_unwind_node_id,
                        ),
                    );
                    self.function_vec.insert(func_id, vec![func_start_node_id]);
                }
            }
        } else {
            for node_idx in self.callgraph.graph.node_indices() {
                // println!("{:?}", self.callgraph.graph.node_weight(node_idx).unwrap());
                let func_instance = self.callgraph.graph.node_weight(node_idx).unwrap();
                let func_id = func_instance.instance().def_id();
                let func_name = self.tcx.def_path_str(func_id);
                if func_name.contains("core")
                    || func_name.contains("std")
                    || func_name.contains("alloc")
                    || func_name.contains("parking_lot::")
                    || func_name.contains("spin::")
                    || func_name.contains("::new")
                {
                    continue;
                }
                let func_start = Place::new_with_no_token(format!("{}", func_name) + "start");
                let func_start_node_id = self.net.add_node(PetriNetNode::P(func_start));
                let func_end = Place::new_with_no_token(format!("{}", func_name) + "end");
                let func_end_node_id = self.net.add_node(PetriNetNode::P(func_end));
                let func_panic = Place::new_with_no_token(format!("{}", func_name) + "panic");
                let func_panic_node_id = self.net.add_node(PetriNetNode::P(func_panic));
                let func_unwind = Place::new_with_no_token(format!("{}", func_name) + "unwind");
                let func_unwind_node_id = self.net.add_node(PetriNetNode::P(func_unwind));
                self.function_counter.insert(
                    func_id,
                    (
                        func_start_node_id,
                        func_end_node_id,
                        func_panic_node_id,
                        func_unwind_node_id,
                    ),
                );
                self.function_vec.insert(func_id, vec![func_start_node_id]);
            }
        }
    }

    // Construct lock for place
    pub fn construct_lock(&mut self, alias_analysis: &mut AliasAnalysis) {
        let lockguards = self.collect_lockguards();
        // classify the lock point to the same memory location
        let mut lockguard_relations = FxHashSet::<(LockGuardId, LockGuardId)>::default();
        let mut info = FxHashMap::default();

        for (_, map) in lockguards.clone().into_iter() {
            info.extend(map.clone().into_iter());
            self.lock_info.extend(map.into_iter());
        }

        println!("The count of locks: {:?}", info.keys().count());
        let mut lock_map: HashMap<LockGuardId, u32> = HashMap::new();
        let mut counter: u32 = 0;
        for (a, _) in info.iter() {
            for (b, _) in info.iter() {
                // lockguard_relations.insert((*k1, *k2));
                if a == b {
                    continue;
                }
                let possibility = self.deadlock_possibility(a, b, &info, alias_analysis);
                match possibility {
                    DeadlockPossibility::Probably | DeadlockPossibility::Possibly => {
                        if !lock_map.contains_key(a) && !lock_map.contains_key(b) {
                            lock_map.insert(*a, counter);
                            lock_map.insert(*b, counter);
                            counter += 1;
                        } else if !lock_map.contains_key(a) {
                            let value = *lock_map.get(b).unwrap();
                            lock_map.insert(*a, value);
                        } else if !lock_map.contains_key(b) {
                            let value = *lock_map.get(a).unwrap();
                            lock_map.insert(*b, value);
                        } else {
                            assert_eq!(*lock_map.get(a).unwrap(), *lock_map.get(b).unwrap());
                        }
                    }
                    _ => {
                        // if !lock_map.contains_key(a) && !lock_map.contains_key(b) {
                        //     lock_map.insert(*a, counter);
                        //     counter += 1;
                        //     lock_map.insert(*b, counter);
                        //     counter += 1;
                        // } else if !lock_map.contains_key(a) {
                        //     lock_map.insert(*a, counter);
                        //     counter += 1;
                        // } else if !lock_map.contains_key(b) {
                        //     lock_map.insert(*b, counter);
                        //     counter += 1;
                        // } else {
                        //     assert_ne!(*lock_map.get(a).unwrap(), *lock_map.get(b).unwrap());
                        // }
                    }
                }
            }
        }

        // for (a, b) in &lockguard_relations {
        //     let possibility = self.deadlock_possibility(a, b, &info, alias_analysis);
        //     match possibility {
        //         DeadlockPossibility::Probably | DeadlockPossibility::Possibly => {
        //             if !lock_map.contains_key(a) && !lock_map.contains_key(b) {
        //                 lock_map.insert(*a, counter);
        //                 lock_map.insert(*b, counter);
        //                 counter += 1;
        //             } else if !lock_map.contains_key(a) {
        //                 let value = *lock_map.get(b).unwrap();
        //                 lock_map.insert(*a, value);
        //             } else if !lock_map.contains_key(b) {
        //                 let value = *lock_map.get(a).unwrap();
        //                 lock_map.insert(*b, value);
        //             } else {
        //                 assert_eq!(*lock_map.get(a).unwrap(), *lock_map.get(b).unwrap());
        //             }
        //         }
        //         _ => {
        //             if !lock_map.contains_key(a) && !lock_map.contains_key(b) {
        //                 lock_map.insert(*a, counter);
        //                 counter += 1;
        //                 lock_map.insert(*b, counter);
        //                 counter += 1;
        //             } else if !lock_map.contains_key(a) {
        //                 lock_map.insert(*a, counter);
        //                 counter += 1;
        //             } else if !lock_map.contains_key(b) {
        //                 lock_map.insert(*b, counter);
        //                 counter += 1;
        //             } else {
        //                 assert_ne!(*lock_map.get(a).unwrap(), *lock_map.get(b).unwrap());
        //             }
        //         }
        //     }
        // }

        let mut lock_id_map: HashMap<u32, Vec<LockGuardId>> = HashMap::new();
        for (lock_id, value) in lock_map {
            if lock_id_map.contains_key(&value) {
                let vec = lock_id_map.get_mut(&value).unwrap();
                vec.push(lock_id);
            } else {
                let mut vec = Vec::new();
                vec.push(lock_id);
                lock_id_map.insert(value, vec);
            }
        }
        println!("The lock_id_map count?: {:?}", lock_id_map.keys().count());

        for (id, lock_vec) in lock_id_map {
            match &info[&lock_vec[0]].lockguard_ty {
                LockGuardTy::StdMutex(_)
                | LockGuardTy::ParkingLotMutex(_)
                | LockGuardTy::SpinMutex(_) => {
                    let lock_name = String::from("Mutex") + &format!("{:?}", id);

                    let lock_p = Place::new(format!("{:?}", lock_name), 1);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    for lock in lock_vec {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
                _ => {
                    let lock_name = String::from("RwLock") + &format!("{:?}", id);
                    let lock_p = Place::new(format!("{:?}", lock_name), 10);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    for lock in lock_vec {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
            }
        }
    }

    pub fn construct_lock_with_dfs(&mut self, alias_analysis: &mut AliasAnalysis) {
        let lockguards = self.collect_lockguards();
        let mut info = FxHashMap::default();

        for (_, map) in lockguards.clone().into_iter() {
            info.extend(map.clone().into_iter());
            self.lock_info.extend(map.into_iter());
        }

        let mut adj_list: HashMap<usize, Vec<usize>> = HashMap::new();
        let lockid_vec: Vec<LockGuardId> = info.clone().into_keys().collect::<Vec<LockGuardId>>();
        println!("{:?}", lockid_vec);
        for i in 0..lockid_vec.len() {
            for j in i + 1..lockid_vec.len() {
                match self.deadlock_possibility(
                    &lockid_vec[i],
                    &lockid_vec[j],
                    &info,
                    alias_analysis,
                ) {
                    DeadlockPossibility::Probably | DeadlockPossibility::Possibly => {
                        adj_list.entry(i).or_insert_with(Vec::new).push(j);
                        adj_list.entry(j).or_insert_with(Vec::new).push(i);
                    }
                    _ => {}
                }
            }
        }
        println!("{:?}", adj_list);
        let mut visited: Vec<bool> = vec![false; lockid_vec.len()];
        let mut group_id = 0;
        let mut groups: HashMap<usize, Vec<LockGuardId>> = HashMap::new();

        for i in 0..lockid_vec.len() {
            if !visited[i] {
                let mut stack: VecDeque<usize> = VecDeque::new();
                stack.push_back(i);
                visited[i] = true;
                while let Some(node) = stack.pop_front() {
                    groups
                        .entry(group_id)
                        .or_insert_with(Vec::new)
                        .push(lockid_vec[node].clone());
                    if let Some(neighbors) = adj_list.get(&node) {
                        for &neighbor in neighbors {
                            if !visited[neighbor] {
                                stack.push_back(neighbor);
                                visited[neighbor] = true;
                            }
                        }
                    }
                }
                group_id += 1;
            }
        }

        for (id, lock_vec) in groups {
            match &info[&lock_vec[0]].lockguard_ty {
                LockGuardTy::StdMutex(_)
                | LockGuardTy::ParkingLotMutex(_)
                | LockGuardTy::SpinMutex(_) => {
                    let lock_name = String::from("Mutex") + &format!("{:?}", id);

                    let lock_p = Place::new(format!("{:?}", lock_name), 1);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    for lock in lock_vec {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
                _ => {
                    let lock_name = String::from("RwLock") + &format!("{:?}", id);
                    let lock_p = Place::new(format!("{:?}", lock_name), 10);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    for lock in lock_vec {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
            }
        }
    }

    fn deal_post_function(&mut self) {
        for (id, func_node_vec) in &self.function_vec {
            if func_node_vec.len() < 3 {
                let start = func_node_vec.first().unwrap();
                let end = func_node_vec.last().unwrap();
                let t = format!("{:?}", id) + &String::from("no_lock");
                let transition = Transition::new(t, (0, 0), 1);
                let t_node = self.net.add_node(PetriNetNode::T(transition));

                self.net.add_edge(*start, t_node, PetriNetEdge { label: 1 });
                self.net.add_edge(t_node, *end, PetriNetEdge { label: 1 });
            }
        }
    }

    fn collect_lockguards(&self) -> FxHashMap<InstanceId, LockGuardMap<'tcx>> {
        let mut lockguards = FxHashMap::default();
        for (instance_id, node) in self.callgraph.graph.node_references() {
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

    fn deadlock_possibility(
        &self,
        a: &LockGuardId,
        b: &LockGuardId,
        lockguards: &LockGuardMap<'tcx>,
        alias_analysis: &mut AliasAnalysis,
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
            DeadlockPossibility::Probably => match alias_analysis.alias((*a).into(), (*b).into()) {
                ApproximateAliasKind::Probably => DeadlockPossibility::Probably,
                ApproximateAliasKind::Possibly => DeadlockPossibility::Possibly,
                ApproximateAliasKind::Unlikely => DeadlockPossibility::Unlikely,
                ApproximateAliasKind::Unknown => DeadlockPossibility::Unknown,
            },
            DeadlockPossibility::Possibly => match alias_analysis.alias((*a).into(), (*b).into()) {
                ApproximateAliasKind::Probably => DeadlockPossibility::Possibly,
                ApproximateAliasKind::Possibly => DeadlockPossibility::Possibly,
                ApproximateAliasKind::Unlikely => DeadlockPossibility::Unlikely,
                ApproximateAliasKind::Unknown => DeadlockPossibility::Unknown,
            },
            _ => DeadlockPossibility::Unlikely,
        };
        possibility
    }

    // Get initial state of Petri net.
    // pub fn get_initial_state(&self) -> State {
    //     let mark = self
    //         .net
    //         .node_indices()
    //         .filter_map(|idx| match &self.net[idx] {
    //             PetriNetNode::P(Place { token: 0, .. }) => Some(&self.net[idx]),
    //             _ => None,
    //         })
    //         .collect();
    //     State::new(mark)
    // }

    // Get all enabled transitions at current state
    pub fn get_sched_transitions(&self) -> Vec<NodeIndex> {
        let mut sched_transiton = Vec::<NodeIndex>::new();
        for node_index in self.net.node_indices() {
            let node_weight = self.net.node_weight(node_index);
            match node_weight {
                Some(node) => match node {
                    PetriNetNode::P(_) => {
                        continue;
                    }
                    PetriNetNode::T(_) => {
                        let mut enabled = true;
                        for edge in self.net.edges_directed(node_index, Direction::Incoming) {
                            let place_node = self.net.node_weight(edge.source()).unwrap();
                            match place_node {
                                PetriNetNode::P(place) => {
                                    if *place.tokens.borrow() < edge.weight().label {
                                        enabled = false;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        if enabled {
                            sched_transiton.push(node_index);
                        }
                    }
                },
                None => println!("Node {}: no weight", node_index.index()),
            }
        }
        sched_transiton
    }

    // Choose a transition to fire
    pub fn fire_transition(
        &mut self,
        transition: NodeIndex,
        mark: HashSet<(NodeIndex, usize)>,
    ) -> HashSet<(NodeIndex, usize)> {
        let mut new_state = HashSet::<(NodeIndex, usize)>::new();
        self.set_current_mark(mark);

        // 从输入库所中减去token
        for edge in self.net.edges_directed(transition, Direction::Incoming) {
            let place_node = self.net.node_weight(edge.source()).unwrap();
            match place_node {
                PetriNetNode::P(place) => {
                    *place.tokens.borrow_mut() -= edge.weight().label;
                }
                PetriNetNode::T(_) => {
                    println!("{}", "this error!");
                }
            }
        }

        // 将token添加到输出库所中
        for edge in self.net.edges_directed(transition, Direction::Outgoing) {
            let place_node = self.net.node_weight(edge.target()).unwrap();
            match place_node {
                PetriNetNode::P(place) => {
                    *place.tokens.borrow_mut() += edge.weight().label;
                }
                PetriNetNode::T(_) => {
                    println!("{}", "this error!");
                }
            }
        }

        for node in self.net.node_indices() {
            match &self.net[node] {
                PetriNetNode::P(place) => {
                    if *place.tokens.borrow() > 0 {
                        new_state.insert((node, *place.tokens.borrow() as usize));
                    }
                }
                PetriNetNode::T(_) => {
                    //println!("{}", "no record");
                }
            }
        }

        new_state
    }

    pub fn add_token(&mut self, place_index: NodeIndex, weight: u32) {
        match &mut self.net[place_index] {
            PetriNetNode::P(place) => {
                *place.tokens.borrow_mut() = *place.tokens.borrow_mut() - weight;
            }
            PetriNetNode::T(_) => {
                println!("{}", "this error!");
            }
        }
    }

    // Generate state graph for Petri net
    // #[cfg(not(feature = "multi-threaded"))]
    pub fn generate_state_graph(&mut self) -> StateGraph {
        let mut state_graph = StateGraph::new();
        let mut queue = VecDeque::<HashSet<(NodeIndex, usize)>>::new();

        let init_mark = self.get_current_mark();
        // let init_index = state_graph
        //     .graph
        //     .add_node(StateNode::new(init_mark.clone()));
        let init_usize = init_mark.iter().map(|node| node.0.index()).collect();
        queue.push_back(init_mark);
        let mut all_state = HashSet::<Vec<usize>>::new();
        all_state.insert(init_usize);

        while let Some(current_state_index) = queue.pop_front() {
            self.set_current_mark(current_state_index.clone());

            let current_sched_transition = self.get_sched_transitions();

            if current_sched_transition.is_empty() {
                // println!("No transitions scheduled");
                let current_state_uszie: Vec<usize> = current_state_index
                    .iter()
                    .map(|node| node.0.index())
                    .collect();
                self.deadlock_marks.insert(current_state_uszie);
                continue;
            } else {
                for t in current_sched_transition {
                    let new_state = self.fire_transition(t, current_state_index.clone());
                    let new_state_uszie: Vec<usize> = new_state
                        .clone()
                        .iter()
                        .map(|node| node.0.index())
                        .collect();

                    if all_state.insert(new_state_uszie) {
                        queue.push_back(new_state.clone());
                    }
                }
            }
        }

        state_graph
    }

    // Check Deadlock
    pub fn check_deadlock(&mut self) {
        use petgraph::graph::node_index;
        // Remove the terminal mark
        self.deadlock_marks.retain(|v| {
            v.iter().all(|m| match &self.net[node_index(*m)] {
                PetriNetNode::P(p) => {
                    if p.name.contains("panic")
                        || p.name.contains("mainunwind")
                        || p.name.contains("mainend")
                    {
                        false
                    } else {
                        true
                    }
                }
                _ => false,
            })
        });
        for mark in &self.deadlock_marks {
            // let joined = mark
            //     .clone()
            //     .iter()
            //     .map(|x| x.to_string())
            //     .collect::<Vec<String>>()
            //     .join(", ");

            let joined = mark
                .clone()
                .iter()
                .map(|x| match &self.net[node_index(*x)] {
                    PetriNetNode::P(p) => p.name.clone(),
                    PetriNetNode::T(t) => t.name.clone(),
                })
                .collect::<Vec<String>>()
                .join(", ");
            println!("{:?}", joined);
        }
    }

    // Set the current marking
    pub fn set_current_mark(&mut self, mark: HashSet<(NodeIndex, usize)>) {
        for node in self.net.node_indices() {
            match &mut self.net[node] {
                PetriNetNode::P(place) => {
                    *place.tokens.borrow_mut() = 0;
                }
                PetriNetNode::T(_) => {
                    debug!("{}", "this error!");
                }
            }
        }
        for (m, n) in mark {
            match &mut self.net[m] {
                PetriNetNode::P(place) => {
                    *place.tokens.borrow_mut() = n as u32;
                }
                PetriNetNode::T(_) => {
                    debug!("{}", "this error!");
                }
            }
        }
    }

    // Get the current marking
    pub fn get_current_mark(&self) -> HashSet<(NodeIndex, usize)> {
        let mut current_mark = HashSet::<(NodeIndex, usize)>::new();
        for node in self.net.node_indices() {
            match &self.net[node] {
                PetriNetNode::P(place) => {
                    if *place.tokens.borrow() != 0 {
                        current_mark.insert((node.clone(), *place.tokens.borrow() as usize));
                    }
                }
                PetriNetNode::T(_) => {
                    debug!("{}", "this error!");
                }
            }
        }
        current_mark
    }
}

/// Collect lockguard info.
pub struct LinkConstruct<'b, 'tcx> {
    instance_id: InstanceId,
    instance: &'b Instance<'tcx>,
    body: &'b Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    param_env: ParamEnv<'tcx>,
    pub net: &'b mut Graph<PetriNetNode, PetriNetEdge>,
    pub lockguards: LockGuardMap<'tcx>,
    function_counter: &'b HashMap<DefId, (NodeIndex, NodeIndex)>,
    pub function_vec: &'b mut HashMap<DefId, Vec<NodeIndex>>,
    locks_counter: &'b HashMap<LockGuardId, NodeIndex>,
}

impl<'b, 'tcx> LinkConstruct<'b, 'tcx> {
    pub fn new(
        instance_id: InstanceId,
        instance: &'b Instance<'tcx>,
        body: &'b Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        param_env: ParamEnv<'tcx>,
        net: &'b mut Graph<PetriNetNode, PetriNetEdge>,
        lockguards: LockGuardMap<'tcx>,
        function_counter: &'b HashMap<DefId, (NodeIndex, NodeIndex)>,
        function_vec: &'b mut HashMap<DefId, Vec<NodeIndex>>,
        locks_counter: &'b HashMap<LockGuardId, NodeIndex>,
    ) -> Self {
        Self {
            instance_id,
            instance,
            body,
            tcx,
            param_env,
            net,
            lockguards,
            function_counter,
            function_vec,
            locks_counter,
        }
    }

    pub fn analyze(&mut self) {
        self.visit_body(self.body);
    }

    pub fn extract_def_id_of_called_function_from_operand(
        operand: &rustc_middle::mir::Operand<'tcx>,
        caller_function_def_id: rustc_hir::def_id::DefId,
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
    ) -> rustc_hir::def_id::DefId {
        let function_type = match operand {
            rustc_middle::mir::Operand::Copy(place) | rustc_middle::mir::Operand::Move(place) => {
                // Find the type through the local declarations of the caller function.
                // The `Place` (memory location) of the called function should be declared there and we can query its type.
                let body = tcx.optimized_mir(caller_function_def_id);
                let place_ty = place.ty(body, tcx);
                place_ty.ty
            }
            rustc_middle::mir::Operand::Constant(constant) => constant.ty(),
        };
        match function_type.kind() {
            rustc_middle::ty::TyKind::FnPtr(_) => {
                unimplemented!(
                    "TyKind::FnPtr not implemented yet. Function pointers are present in the MIR"
                );
            }
            rustc_middle::ty::TyKind::FnDef(def_id, _)
            | rustc_middle::ty::TyKind::Closure(def_id, _) => *def_id,
            _ => {
                panic!("TyKind::FnDef, a function definition, but got: {function_type:?}");
            }
        }
    }
}

impl<'b, 'tcx> Visitor<'tcx> for LinkConstruct<'b, 'tcx> {
    fn visit_local(&mut self, local: Local, context: PlaceContext, location: Location) {
        let lockguard_id = LockGuardId::new(self.instance_id, local);
        // local is lockguard
        if let Some(info) = self.lockguards.get_mut(&lockguard_id) {
            match context {
                PlaceContext::MutatingUse(context) => match context {
                    MutatingUseContext::Drop => {
                        let lock_node = self.locks_counter.get(&lockguard_id).unwrap();
                        let drop_t = format!("{:?}", lockguard_id.instance_id)
                            + &String::from("drop")
                            + &format!("{:?}", lock_node.index());
                        let drop_p = format!("{:?}", lockguard_id.instance_id)
                            + &String::from("dropped")
                            + &format!("{:?}", lock_node.index());
                        let drop_lock_t = Transition::new(format!("{:?}", drop_t), (0, 0), 1);
                        let drop_lock_p = Place::new_with_no_token(format!("{:?}", drop_p));
                        let drop_node_t = self.net.add_node(PetriNetNode::T(drop_lock_t));
                        let drop_node_p = self.net.add_node(PetriNetNode::P(drop_lock_p));

                        let prev_node = self.function_vec[&self.instance.def_id()].last().unwrap();
                        self.net
                            .add_edge(*prev_node, drop_node_t, PetriNetEdge { label: 1u32 });
                        self.net
                            .add_edge(drop_node_t, drop_node_p, PetriNetEdge { label: 1u32 });
                        match &self.lockguards[&lockguard_id].lockguard_ty {
                            LockGuardTy::StdMutex(_)
                            | LockGuardTy::ParkingLotMutex(_)
                            | LockGuardTy::SpinMutex(_) => {
                                self.net.add_edge(
                                    drop_node_t,
                                    *lock_node,
                                    PetriNetEdge { label: 1u32 },
                                );
                            }
                            _ => {
                                self.net.add_edge(
                                    drop_node_t,
                                    *lock_node,
                                    PetriNetEdge { label: 10u32 },
                                );
                            }
                        }
                    }
                    MutatingUseContext::Call => {
                        let lock_node = self.locks_counter.get(&lockguard_id).unwrap();
                        let genc_t = format!("{:?}", lockguard_id.instance_id)
                            + &String::from("genc")
                            + &format!("{:?}", lock_node.index());
                        let genc_p = format!("{:?}", lockguard_id.instance_id)
                            + &String::from("locked")
                            + &format!("{:?}", lock_node.index());
                        let genc_lock_t = Transition::new(format!("{:?}", genc_t), (0, 0), 1);
                        let genc_lock_p = Place::new_with_no_token(format!("{:?}", genc_p));
                        let genc_node_t = self.net.add_node(PetriNetNode::T(genc_lock_t));
                        let genc_node_p = self.net.add_node(PetriNetNode::P(genc_lock_p));

                        let prev_node = self.function_vec[&self.instance.def_id()].last().unwrap();
                        self.net
                            .add_edge(*prev_node, genc_node_t, PetriNetEdge { label: 1u32 });
                        self.net
                            .add_edge(genc_node_t, genc_node_p, PetriNetEdge { label: 1u32 });
                        match &self.lockguards[&lockguard_id].lockguard_ty {
                            LockGuardTy::StdMutex(_)
                            | LockGuardTy::ParkingLotMutex(_)
                            | LockGuardTy::SpinMutex(_) => {
                                self.net.add_edge(
                                    *lock_node,
                                    genc_node_t,
                                    PetriNetEdge { label: 1u32 },
                                );
                            }
                            _ => {
                                self.net.add_edge(
                                    *lock_node,
                                    genc_node_t,
                                    PetriNetEdge { label: 10u32 },
                                );
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        if let TerminatorKind::Call {
            ref func, fn_span, ..
        } = terminator.kind
        {
            let func_ty = func.ty(self.body, self.tcx);

            if let ty::FnDef(def_id, substs) = *func_ty.kind() {
                let func_name = self.tcx.def_path_str(def_id);
                if func_name.contains("core")
                    || func_name.contains("std")
                    || func_name.contains("alloc")
                    || func_name.contains("parking_lot::")
                    || func_name.contains("spin::")
                    || func_name.contains("::new")
                {
                } else {
                    if let Some(callee) =
                        Instance::resolve(self.tcx, self.param_env, def_id, substs)
                            .ok()
                            .flatten()
                    {
                        let func_name = self.tcx.def_path_str(def_id);
                        println!("{:?}", def_id);
                        let func_start_end = self.function_counter.get(&def_id);
                        match func_start_end {
                            Some(_) => {
                                if func_name == "std::mem::drop"
                                    || func_name == "std::ops::Deref::deref"
                                    || func_name == "std::ops::DerefMut::deref_mut"
                                    || func_name == "std::result::Result::<T, E>::unwrap"
                                {
                                    return;
                                }
                                if func_name == "std::thread::spawn" {
                                    // thread
                                    return;
                                }
                                let call = format!("{:?}", fn_span)
                                    + &String::from("call")
                                    + &format!("{:?}", def_id);
                                let wait = format!("{:?}", fn_span)
                                    + &String::from("wait")
                                    + &format!("{:?}", def_id);
                                let ret = format!("{:?}", fn_span)
                                    + &String::from("return")
                                    + &format!("{:?}", def_id);
                                let called = format!("{:?}", fn_span)
                                    + &String::from("called")
                                    + &format!("{:?}", def_id);
                                let call_t = Transition::new(call, (0, 0), 1);
                                let wait_p = Place::new_with_no_token(wait);
                                let ret_t = Transition::new(ret, (0, 0), 1);
                                let call_p = Place::new_with_no_token(called);

                                let call_node_t = self.net.add_node(PetriNetNode::T(call_t));
                                let wait_node_p = self.net.add_node(PetriNetNode::P(wait_p));
                                let ret_node_t = self.net.add_node(PetriNetNode::T(ret_t));
                                let call_node_p = self.net.add_node(PetriNetNode::P(call_p));

                                let prev_node = self.function_vec[&def_id].last().unwrap();

                                self.net.add_edge(
                                    *prev_node,
                                    call_node_t,
                                    PetriNetEdge { label: 1u32 },
                                );
                                self.net.add_edge(
                                    call_node_t,
                                    wait_node_p,
                                    PetriNetEdge { label: 1u32 },
                                );
                                self.net.add_edge(
                                    wait_node_p,
                                    ret_node_t,
                                    PetriNetEdge { label: 1u32 },
                                );
                                self.net.add_edge(
                                    ret_node_t,
                                    call_node_p,
                                    PetriNetEdge { label: 1u32 },
                                );
                                self.function_vec
                                    .get_mut(&def_id)
                                    .unwrap()
                                    .push(call_node_t);
                                self.function_vec
                                    .get_mut(&def_id)
                                    .unwrap()
                                    .push(wait_node_p);
                                self.function_vec.get_mut(&def_id).unwrap().push(ret_node_t);
                                self.function_vec
                                    .get_mut(&def_id)
                                    .unwrap()
                                    .push(call_node_p);
                                self.net.add_edge(
                                    call_node_t,
                                    func_start_end.unwrap().0,
                                    PetriNetEdge { label: 1u32 },
                                );
                                self.net.add_edge(
                                    func_start_end.unwrap().1,
                                    ret_node_t,
                                    PetriNetEdge { label: 1u32 },
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        self.super_terminator(terminator, location);
    }
}
