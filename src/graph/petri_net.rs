use crate::Options;
use log::debug;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::visit::IntoNodeReferences;
use petgraph::Direction;
use petgraph::Graph;
use rustc_hash::{FxHashMap, FxHashSet};
use rustc_hir::def_id::DefId;
use rustc_middle::{
    mir::{
        visit::{MutatingUseContext, PlaceContext, Visitor},
        Body, Local, Location, Terminator, TerminatorKind,
    },
    ty::{self, Instance, ParamEnv, TyCtxt},
};
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::hash::Hash;

use super::callgraph::{CallGraph, CallGraphNode, InstanceId};
use super::function_pn::FunctionPN;
use super::state_graph::StateGraph;
use crate::concurrency::candvar::CondVarCollector;
use crate::concurrency::candvar::CondVarId;
use crate::concurrency::candvar::CondVarInfo;
use crate::concurrency::handler::JoinHanderId;
use crate::concurrency::handler::JoinHandlerCollector;
use crate::concurrency::handler::JoinHandlerMap;
use crate::graph::state_graph::StateEdge;
use crate::graph::state_graph::StateNode;
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
    pub name: String,
    pub tokens: RefCell<usize>,
    pub capacity: usize,
    shape: Shape,
    terminal_mark: bool,
    pub details: String,
}

impl Place {
    pub fn new(name: String, token: usize) -> Self {
        Self {
            name,
            tokens: RefCell::new(token),
            capacity: token,
            shape: Shape::Circle,
            terminal_mark: false,
            details: String::new(),
        }
    }

    pub fn new_with_no_token(name: String) -> Self {
        Self {
            name,
            tokens: RefCell::new(0usize),
            capacity: 1usize,
            shape: Shape::Circle,
            terminal_mark: false,
            details: String::new(),
        }
    }

    pub fn new_with_terminal_mark(name: String, token: usize, terminal_mark: bool) -> Self {
        Self {
            name,
            tokens: RefCell::new(token),
            capacity: 1,
            shape: Shape::Circle,
            terminal_mark,
            details: String::new(),
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
    pub name: String,
    time: (u32, u32),
    pub weight: u32,
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
    pub label: usize,
}

impl std::fmt::Display for PetriNetEdge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

fn insert_with_comparison<T: Eq + Hash>(set: &mut HashSet<T>, value: T) -> bool {
    for existing_value in set.iter() {
        if existing_value == &value {
            return false;
        }
    }
    set.insert(value);
    return true;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marking {
    marks: HashMap<NodeIndex, usize>, // NodeIndex represents the place, usize represents token count
}

impl Hash for Marking {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for (key, value) in &self.marks {
            key.hash(state);
            value.hash(state);
        }
    }
}

pub struct PetriNet<'compilation, 'a, 'tcx> {
    options: &'compilation Options,
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    param_env: ParamEnv<'tcx>,
    pub net: Graph<PetriNetNode, PetriNetEdge>,
    callgraph: &'a CallGraph<'tcx>,
    alias: RefCell<AliasAnalysis<'a, 'tcx>>,
    function_counter: HashMap<DefId, (NodeIndex, NodeIndex)>,
    pub function_vec: HashMap<DefId, Vec<NodeIndex>>,
    locks_counter: HashMap<LockGuardId, NodeIndex>,
    lock_info: LockGuardMap<'tcx>,
    deadlock_marks: HashSet<Vec<(usize, usize)>>,
    // thread id and handler
    thread_id_handler: HashMap<usize, Vec<JoinHanderId>>,
    handler_id: HashMap<JoinHanderId, DefId>,
    // all condvars
    condvars: HashMap<CondVarId, NodeIndex>,
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
impl<'compilation, 'a, 'tcx> PetriNet<'compilation, 'a, 'tcx> {
    pub fn new(
        options: &'compilation Options,
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
        param_env: ParamEnv<'tcx>,
        callgraph: &'a CallGraph<'tcx>,
    ) -> Self {
        let alias = RefCell::new(AliasAnalysis::new(tcx, &callgraph));
        Self {
            options,
            tcx,
            param_env,
            net: Graph::<PetriNetNode, PetriNetEdge>::new(),
            callgraph,
            alias,
            function_counter: HashMap::<DefId, (NodeIndex, NodeIndex)>::new(),
            function_vec: HashMap::<DefId, Vec<NodeIndex>>::new(),
            locks_counter: HashMap::<LockGuardId, NodeIndex>::new(),
            lock_info: HashMap::default(),
            deadlock_marks: HashSet::<Vec<(usize, usize)>>::new(),
            thread_id_handler: HashMap::<usize, Vec<JoinHanderId>>::new(),
            handler_id: HashMap::<JoinHanderId, DefId>::new(),
            condvars: HashMap::<CondVarId, NodeIndex>::new(),
        }
    }

    pub fn construct(&mut self /*alias_analysis: &'a RefCell<AliasAnalysis<'a, 'tcx>>*/) {
        self.construct_func();
        self.construct_lock_with_dfs();
        self.collect_handle();
        self.collect_condvar();
        for (node, caller) in self.callgraph.graph.node_references() {
            if self.tcx.is_mir_available(caller.instance().def_id())
                && self
                    .format_name(caller.instance().def_id())
                    .contains(&self.options.crate_name)
            {
                self.visitor_function_body(node, caller);
            }
        }
        self.reduce_state();
    }

    /// Extracts a function name from the DefId of a function.
    fn format_name(&self, def_id: DefId) -> String {
        let tmp1 = format!("{def_id:?}");
        let tmp2: &str = tmp1.split("~ ").collect::<Vec<&str>>()[1];
        let tmp3 = tmp2.replace(')', "");
        let lhs = tmp3.split('[').collect::<Vec<&str>>()[0];
        let rhs = tmp3.split(']').collect::<Vec<&str>>()[1];
        format!("{lhs}{rhs}").to_string()
    }

    pub fn visitor_function_body(
        &mut self,
        node: NodeIndex,
        caller: &CallGraphNode<'tcx>,
        //alias_analysis: &'a RefCell<AliasAnalysis<'a, 'tcx>>,
    ) {
        let body = self.tcx.optimized_mir(caller.instance().def_id());
        // let body = self.tcx.instance_mir(caller.instance().def);
        // Skip promoted src
        if body.source.promoted.is_some() {
            return;
        }
        let lock_infos = self.lock_info.clone();

        let mut func_construct = FunctionPN::new(
            node,
            caller.instance(),
            body,
            self.tcx,
            // self.param_env,
            &mut self.net,
            &self.alias,
            lock_infos,
            &self.function_counter,
            &self.locks_counter,
            &mut self.thread_id_handler,
            &mut self.handler_id,
            &self.condvars,
        );
        func_construct.analyze();
    }

    // Construct Function Start and End Place by callgraph
    pub fn construct_func(&mut self) {
        if let Some((main_func, _)) = self.tcx.entry_fn(()) {
            for node_idx in self.callgraph.graph.node_indices() {
                // println!("{:?}", self.callgraph.graph.node_weight(node_idx).unwrap());
                let func_instance = self.callgraph.graph.node_weight(node_idx).unwrap();
                let func_id = func_instance.instance().def_id();
                let func_name = self.tcx.def_path_str(func_id);
                // if func_name.contains("core")
                //     || func_name.contains("std")
                //     || func_name.contains("alloc")
                //     || func_name.contains("parking_lot::")
                //     || func_name.contains("spin::")
                //     || func_name.contains("::new")
                //     || func_name.contains("libc")
                // {
                //     continue;
                // }
                if func_id == main_func {
                    let func_start = Place::new(format!("{}", func_name) + "start", 1);
                    let func_start_node_id = self.net.add_node(PetriNetNode::P(func_start));
                    let func_end = Place::new_with_no_token(format!("{}", func_name) + "end");
                    let func_end_node_id = self.net.add_node(PetriNetNode::P(func_end));

                    self.function_counter
                        .insert(func_id, (func_start_node_id, func_end_node_id));
                    // self.function_vec.push(func_start_node_id);
                    self.function_vec.insert(func_id, vec![func_start_node_id]);
                } else {
                    let func_start = Place::new_with_no_token(format!("{}", func_name) + "start");
                    let func_start_node_id = self.net.add_node(PetriNetNode::P(func_start));
                    let func_end = Place::new_with_no_token(format!("{}", func_name) + "end");
                    let func_end_node_id = self.net.add_node(PetriNetNode::P(func_end));
                    // println!("function id: {:?}", func_id);
                    self.function_counter
                        .insert(func_id, (func_start_node_id, func_end_node_id));
                    self.function_vec.insert(func_id, vec![func_start_node_id]);
                }
            }
        } else {
            log::error!("cargo pta need a entry point!");
            // for node_idx in self.callgraph.graph.node_indices() {
            //     // println!("{:?}", self.callgraph.graph.node_weight(node_idx).unwrap());
            //     let func_instance = self.callgraph.graph.node_weight(node_idx).unwrap();
            //     let func_id = func_instance.instance().def_id();
            //     let func_name = self.tcx.def_path_str(func_id);
            //     if func_name.contains("core")
            //         || func_name.contains("std")
            //         || func_name.contains("alloc")
            //         || func_name.contains("parking_lot::")
            //         || func_name.contains("spin::")
            //         || func_name.contains("::new")
            //         || func_name.contains("libc")
            //     {
            //         continue;
            //     }
            //     let func_start = Place::new_with_no_token(format!("{}", func_name) + "start");
            //     let func_start_node_id = self.net.add_node(PetriNetNode::P(func_start));
            //     let func_end = Place::new_with_no_token(format!("{}", func_name) + "end");
            //     let func_end_node_id = self.net.add_node(PetriNetNode::P(func_end));

            //     self.function_counter
            //         .insert(func_id, (func_start_node_id, func_end_node_id));
            //     self.function_vec.insert(func_id, vec![func_start_node_id]);
            // }
        }
    }

    // Construct lock for place
    pub fn construct_lock(&mut self, alias_analysis: &RefCell<AliasAnalysis>) {
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
                    let lock_name = "Mutex".to_string() + &format!("{:?}", id);

                    let lock_p = Place::new(format!("{:?}", lock_name), 1);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    for lock in lock_vec {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
                _ => {
                    let lock_name = "RwLock".to_string() + &format!("{:?}", id);
                    let lock_p = Place::new(format!("{:?}", lock_name), 10);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    for lock in lock_vec {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
            }
        }
    }

    pub fn construct_lock_with_dfs(&mut self /*alias_analysis: &RefCell<AliasAnalysis>*/) {
        let lockguards = self.collect_lockguards();
        let mut info = FxHashMap::default();

        for (_, map) in lockguards.clone().into_iter() {
            info.extend(map.clone().into_iter());
            self.lock_info.extend(map.into_iter());
        }

        let mut adj_list: HashMap<usize, Vec<usize>> = HashMap::new();
        let lockid_vec: Vec<LockGuardId> = info.clone().into_keys().collect::<Vec<LockGuardId>>();
        debug!("{:?}", lockid_vec);
        for i in 0..lockid_vec.len() {
            for j in i + 1..lockid_vec.len() {
                match self.deadlock_possibility(&lockid_vec[i], &lockid_vec[j], &info, &self.alias)
                {
                    DeadlockPossibility::Probably | DeadlockPossibility::Possibly => {
                        adj_list.entry(i).or_insert_with(Vec::new).push(j);
                        adj_list.entry(j).or_insert_with(Vec::new).push(i);
                    }
                    _ => {}
                }
            }
        }
        debug!("{:?}", adj_list);
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
                    let lock_name = format!("{:?}", "mutex".to_string() + id.to_string().as_str());

                    let lock_p = Place::new(lock_name, 1);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    for lock in lock_vec {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
                _ => {
                    let lock_name = format!("{:?}", "rwlock".to_string() + id.to_string().as_str());
                    let lock_p = Place::new(lock_name, 10);
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

    // Mapping JoinHandle To Thread DefId
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

    fn collect_handle(
        &mut self,
        //alias_analysis: &RefCell<AliasAnalysis>,
    ) -> HashMap<InstanceId, JoinHandlerMap<'tcx>> {
        let mut handlers = HashMap::default();
        for (instance_id, node) in self.callgraph.graph.node_references() {
            let instance = match node {
                CallGraphNode::WithBody(instance) => instance,
                _ => continue,
            };

            if !instance.def_id().is_local() {
                continue;
            }

            let body = self.tcx.instance_mir(instance.def);
            let mut handle_collector =
                JoinHandlerCollector::new(instance_id, instance, body, self.tcx, self.param_env);
            handle_collector.analyze();
            if !handle_collector.handlers.is_empty() {
                handlers.insert(instance_id, handle_collector.handlers);
            }
        }
        let mut info = FxHashMap::default();

        for (_, map) in handlers.clone().into_iter() {
            info.extend(map.clone().into_iter());
        }

        let mut adj_list: HashMap<usize, Vec<usize>> = HashMap::new();
        let lockid_vec: Vec<JoinHanderId> = info.clone().into_keys().collect::<Vec<JoinHanderId>>();
        debug!("{:?}", lockid_vec);
        for i in 0..lockid_vec.len() {
            for j in i + 1..lockid_vec.len() {
                match self
                    .alias
                    .borrow_mut()
                    .alias_handle(lockid_vec[i].clone().into(), lockid_vec[j].clone().into())
                {
                    ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                        adj_list.entry(i).or_insert_with(Vec::new).push(j);
                        adj_list.entry(j).or_insert_with(Vec::new).push(i);
                    }
                    _ => {}
                }
            }
        }
        debug!("{:?}", adj_list);
        let mut visited: Vec<bool> = vec![false; lockid_vec.len()];
        let mut group_id = 0;
        // let mut groups: HashMap<usize, Vec<JoinHanderId>> = HashMap::new();

        for i in 0..lockid_vec.len() {
            if !visited[i] {
                let mut stack: VecDeque<usize> = VecDeque::new();
                stack.push_back(i);
                visited[i] = true;
                while let Some(node) = stack.pop_front() {
                    self.thread_id_handler
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

        handlers
    }

    fn collect_condvar(&mut self) {
        let mut condvars: FxHashMap<NodeIndex, HashMap<CondVarId, CondVarInfo>> =
            FxHashMap::default();
        for (instance_id, node) in self.callgraph.graph.node_references() {
            let instance = match node {
                CallGraphNode::WithBody(instance) => instance,
                _ => continue,
            };

            if !instance.def_id().is_local() {
                continue;
            }

            let body = self.tcx.instance_mir(instance.def);
            let mut condvar_collector =
                CondVarCollector::new(instance_id, instance, body, self.tcx, self.param_env);
            condvar_collector.analyze();
            if !condvar_collector.condvars.is_empty() {
                condvars.insert(instance_id, condvar_collector.condvars);
            }
        }

        // create node for all condvars
        if !condvars.is_empty() {
            for condvar_map in condvars.into_values() {
                for condvar in condvar_map.into_iter() {
                    let condvar_name = format!("condvar:{:?}", condvar.1.span);
                    let condvar_p = Place::new(condvar_name, 1);
                    let condvar_node = self.net.add_node(PetriNetNode::P(condvar_p));
                    self.condvars.insert(condvar.0.clone(), condvar_node);
                }
            }
        }
    }

    fn deadlock_possibility(
        &self,
        a: &LockGuardId,
        b: &LockGuardId,
        lockguards: &LockGuardMap<'tcx>,
        alias_analysis: &RefCell<AliasAnalysis>,
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
                            match self.net.node_weight(edge.source()).unwrap() {
                                PetriNetNode::P(place) => {
                                    if *(place.tokens.borrow()) < edge.weight().label {
                                        enabled = false;
                                        break;
                                    }
                                }
                                _ => {
                                    println!("The predecessor set of transition is not place");
                                }
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
            match self.net.node_weight(edge.source()).unwrap() {
                PetriNetNode::P(place) => {
                    *(place.tokens.borrow_mut()) -= edge.weight().label;
                    // assert!(*place.tokens.borrow() >= 0);
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
                    *(place.tokens.borrow_mut()) += edge.weight().label;
                    if *(place.tokens.borrow()) > place.capacity {
                        *(place.tokens.borrow_mut()) = place.capacity
                    }
                }
                PetriNetNode::T(_) => {
                    println!("{}", "this error!");
                }
            }
        }

        for node in self.net.node_indices() {
            match &self.net[node] {
                PetriNetNode::P(place) => {
                    if *(place.tokens.borrow()) > 0 {
                        new_state.insert((node, *place.tokens.borrow()));
                    }
                }
                PetriNetNode::T(_) => {
                    //println!("{}", "no record");
                }
            }
        }

        new_state
    }

    pub fn add_token(&mut self, place_index: NodeIndex, weight: usize) {
        match &mut self.net[place_index] {
            PetriNetNode::P(place) => {
                *(place.tokens.borrow_mut()) = *(place.tokens.borrow_mut()) - weight;
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
        let mut init_usize: Vec<(usize, usize)> = init_mark
            .clone()
            .iter()
            .map(|node| (node.0.index(), node.1))
            .collect();
        let state_node: Vec<(NodeIndex, usize)> = init_mark
            .clone()
            .iter()
            .map(|node| (node.0, node.1))
            .collect();
        queue.push_back(init_mark);

        let mut all_state = HashSet::<Vec<(usize, usize)>>::new();
        init_usize.sort();

        all_state.insert(init_usize);
        let mut state_node_map: RefCell<HashMap<Vec<(NodeIndex, usize)>, NodeIndex>> =
            RefCell::new(HashMap::new());
        let init_node = state_graph
            .graph
            .add_node(StateNode::new(state_node.clone()));
        state_node_map.borrow_mut().insert(state_node, init_node);
        while let Some(current_state_index) = queue.pop_front() {
            self.set_current_mark(current_state_index.clone());
            let current_node: Vec<(NodeIndex, usize)> = current_state_index
                .clone()
                .iter()
                .map(|node| (node.0, node.1))
                .collect();
            let current_node = state_node_map.borrow().get(&current_node).unwrap().clone();
            let current_sched_transition = self.get_sched_transitions();

            if current_sched_transition.is_empty() {
                // println!("No transitions scheduled");
                let mut current_state_usize: Vec<(usize, usize)> = current_state_index
                    .clone()
                    .iter()
                    .map(|node| (node.0.index(), node.1))
                    .collect();
                current_state_usize.sort();
                self.deadlock_marks.insert(current_state_usize);
                continue;
            } else {
                for t in current_sched_transition {
                    let new_state = self.fire_transition(t, current_state_index.clone());
                    let mut new_state_usize: Vec<(usize, usize)> = new_state
                        .clone()
                        .iter()
                        .map(|node| (node.0.index(), node.1))
                        .collect();
                    new_state_usize.sort();
                    // TODO: Implement for State Graph
                    if insert_with_comparison(&mut all_state, new_state_usize) {
                        queue.push_back(new_state.clone());
                        let state_node: Vec<(NodeIndex, usize)> = new_state
                            .clone()
                            .iter()
                            .map(|node| (node.0, node.1))
                            .collect();
                        let new_node = state_graph.graph.add_node(StateNode {
                            mark: state_node.clone(),
                        });
                        state_graph.graph.add_edge(
                            current_node,
                            new_node,
                            StateEdge::new(format!("{:?}", self.net[t]), 0),
                        );
                        state_node_map.borrow_mut().insert(state_node, new_node);
                    }
                }
            }
        }
        // info!("All states are: {:?}", all_state.len());
        use petgraph::dot::Dot;
        use std::io::Write;
        let mut sg_file = std::fs::File::create("sg.dot").unwrap();

        write!(
            sg_file,
            "{:?}",
            Dot::with_attr_getters(
                &state_graph.graph,
                &[],
                &|_, _| "arrowhead = vee".to_string(),
                &|_, nr| {
                    format!(
                        "label = {:?}",
                        "\"".to_string()
                            + &nr
                                .1
                                .mark
                                .clone()
                                .iter()
                                .map(|x| match &self.net[x.0] {
                                    PetriNetNode::P(p) =>
                                        p.name.clone() + ":" + (x.1).to_string().as_str(),
                                    PetriNetNode::T(t) => t.name.clone(),
                                })
                                .collect::<Vec<String>>()
                                .join(", ")
                            + "\""
                    )
                },
            )
        )
        .unwrap();
        state_graph
    }

    // Check Deadlock
    pub fn check_deadlock(&mut self) {
        use petgraph::graph::node_index;
        // Remove the terminal mark
        self.deadlock_marks.retain(|v| {
            v.iter().all(|m| match &self.net[node_index(m.0)] {
                PetriNetNode::P(p) => {
                    // p.name.contains("mainpanic") ||
                    if p.name.contains("mainend") {
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
                .map(|x| match &self.net[node_index(x.0)] {
                    PetriNetNode::P(p) => p.name.clone() + ":" + (x.1).to_string().as_str(),
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
                    *place.tokens.borrow_mut() = n;
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
                    if *place.tokens.borrow() > 0 {
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

    // Reduce the size of the Petri net and merge edges without branches
    pub fn reduce_state(&mut self) {
        // 删除孤独节点
        // 收集孤立节点
        let mut isolated_nodes = Vec::new();
        for node in self.net.node_indices() {
            if self
                .net
                .edges_directed(node, petgraph::Direction::Incoming)
                .count()
                == 0
                && self
                    .net
                    .edges_directed(node, petgraph::Direction::Outgoing)
                    .count()
                    == 0
            {
                isolated_nodes.push(node);
            }
        }

        // 删除孤立节点
        for node in isolated_nodes {
            self.net.remove_node(node);
        }
    }

    pub fn get_or_insert_node(&mut self, def_id: DefId) -> (NodeIndex, NodeIndex) {
        match self.function_counter.entry(def_id) {
            Entry::Occupied(node) => node.get().to_owned(),
            Entry::Vacant(v) => {
                let func_name = self.tcx.def_path_str(def_id);
                let func_start = Place::new_with_no_token(format!("{}", func_name) + "start");
                let func_start_node_id = self.net.add_node(PetriNetNode::P(func_start));
                let func_end = Place::new_with_no_token(format!("{}", func_name) + "end");
                let func_end_node_id = self.net.add_node(PetriNetNode::P(func_end));
                *v.insert((func_start_node_id, func_end_node_id))
            }
        }
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

    // pub fn extract_def_id_of_called_function_from_operand(
    //     operand: &rustc_middle::mir::Operand<'tcx>,
    //     caller_function_def_id: rustc_hir::def_id::DefId,
    //     tcx: rustc_middle::ty::TyCtxt<'tcx>,
    // ) -> rustc_hir::def_id::DefId {
    //     let function_type = match operand {
    //         rustc_middle::mir::Operand::Copy(place) | rustc_middle::mir::Operand::Move(place) => {
    //             // Find the type through the local declarations of the caller function.
    //             // The `Place` (memory location) of the called function should be declared there and we can query its type.
    //             let body = tcx.optimized_mir(caller_function_def_id);
    //             let place_ty = place.ty(body, tcx);
    //             place_ty.ty
    //         }
    //         rustc_middle::mir::Operand::Constant(constant) => constant.ty(),
    //     };
    //     match function_type.kind() {
    //         rustc_middle::ty::TyKind::FnPtr(_) => {
    //             unimplemented!(
    //                 "TyKind::FnPtr not implemented yet. Function pointers are present in the MIR"
    //             );
    //         }
    //         rustc_middle::ty::TyKind::FnDef(def_id, _)
    //         | rustc_middle::ty::TyKind::Closure(def_id, _) => *def_id,
    //         _ => {
    //             panic!("TyKind::FnDef, a function definition, but got: {function_type:?}");
    //         }
    //     }
    // }
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
                            .add_edge(*prev_node, drop_node_t, PetriNetEdge { label: 1usize });
                        self.net
                            .add_edge(drop_node_t, drop_node_p, PetriNetEdge { label: 1usize });
                        match &self.lockguards[&lockguard_id].lockguard_ty {
                            LockGuardTy::StdMutex(_)
                            | LockGuardTy::ParkingLotMutex(_)
                            | LockGuardTy::SpinMutex(_) => {
                                self.net.add_edge(
                                    drop_node_t,
                                    *lock_node,
                                    PetriNetEdge { label: 1usize },
                                );
                            }
                            _ => {
                                self.net.add_edge(
                                    drop_node_t,
                                    *lock_node,
                                    PetriNetEdge { label: 10usize },
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
                            .add_edge(*prev_node, genc_node_t, PetriNetEdge { label: 1usize });
                        self.net
                            .add_edge(genc_node_t, genc_node_p, PetriNetEdge { label: 1usize });
                        match &self.lockguards[&lockguard_id].lockguard_ty {
                            LockGuardTy::StdMutex(_)
                            | LockGuardTy::ParkingLotMutex(_)
                            | LockGuardTy::SpinMutex(_) => {
                                self.net.add_edge(
                                    *lock_node,
                                    genc_node_t,
                                    PetriNetEdge { label: 1usize },
                                );
                            }
                            _ => {
                                self.net.add_edge(
                                    *lock_node,
                                    genc_node_t,
                                    PetriNetEdge { label: 10usize },
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
                                    PetriNetEdge { label: 1usize },
                                );
                                self.net.add_edge(
                                    call_node_t,
                                    wait_node_p,
                                    PetriNetEdge { label: 1usize },
                                );
                                self.net.add_edge(
                                    wait_node_p,
                                    ret_node_t,
                                    PetriNetEdge { label: 1usize },
                                );
                                self.net.add_edge(
                                    ret_node_t,
                                    call_node_p,
                                    PetriNetEdge { label: 1usize },
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
                                    PetriNetEdge { label: 1usize },
                                );
                                self.net.add_edge(
                                    func_start_end.unwrap().1,
                                    ret_node_t,
                                    PetriNetEdge { label: 1usize },
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
