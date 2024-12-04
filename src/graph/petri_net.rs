use crate::graph::state_graph::normalize_state;
use crate::graph::state_graph::StateEdge;
use crate::graph::state_graph::StateNode;
use crate::utils::format_name;
use crate::Options;
use log::debug;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::visit::IntoNodeReferences;
use petgraph::Direction;
use petgraph::Graph;
use rustc_hash::FxHashMap;
use rustc_hir::def_id::DefId;
use rustc_middle::ty::ParamEnv;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::hash::Hash;
use std::io::Write;
use std::sync::{Arc, RwLock};

use super::callgraph::{CallGraph, CallGraphNode, InstanceId};
use super::function_pn::BodyToPetriNet;
use super::state_graph::StateGraph;
use crate::concurrency::candvar::CondVarCollector;
use crate::concurrency::candvar::CondVarId;
use crate::concurrency::candvar::CondVarInfo;
use crate::concurrency::handler::JoinHanderId;
use crate::concurrency::handler::JoinHandlerCollector;
use crate::concurrency::handler::JoinHandlerMap;
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
    pub tokens: Arc<RwLock<usize>>,
    pub capacity: usize,
    pub span: String,
    pub details: String,
}

impl Place {
    pub fn new(name: String, token: usize) -> Self {
        Self {
            name,
            tokens: Arc::new(RwLock::new(token)),
            capacity: token,
            span: String::new(),
            details: String::new(),
        }
    }

    pub fn new_with_span(name: String, token: usize, span: String) -> Self {
        Self {
            name,
            tokens: Arc::new(RwLock::new(token)),
            capacity: 1usize,
            span,
            details: String::new(),
        }
    }

    pub fn new_with_no_token(name: String) -> Self {
        Self {
            name,
            tokens: Arc::new(RwLock::new(0)),
            capacity: 1usize,
            span: String::new(),
            details: String::new(),
        }
    }
}

impl std::fmt::Display for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[allow(unused)]
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

pub struct PetriNet<'compilation, 'pn, 'tcx> {
    options: &'compilation Options,
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    param_env: ParamEnv<'tcx>,
    pub net: Graph<PetriNetNode, PetriNetEdge>,
    callgraph: &'pn CallGraph<'tcx>,
    alias: RefCell<AliasAnalysis<'pn, 'tcx>>,
    function_counter: HashMap<DefId, (NodeIndex, NodeIndex)>,
    pub function_vec: HashMap<DefId, Vec<NodeIndex>>,
    locks_counter: HashMap<LockGuardId, NodeIndex>,
    lock_info: LockGuardMap<'tcx>,
    // thread id and handler
    thread_id_handler: HashMap<usize, Vec<JoinHanderId>>,
    handler_id: HashMap<JoinHanderId, DefId>,
    // all condvars
    condvars: HashMap<CondVarId, NodeIndex>,
    pub entry_node: NodeIndex,
}

impl<'compilation, 'pn, 'tcx> PetriNet<'compilation, 'pn, 'tcx> {
    pub fn new(
        options: &'compilation Options,
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
        param_env: ParamEnv<'tcx>,
        callgraph: &'pn CallGraph<'tcx>,
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
            thread_id_handler: HashMap::<usize, Vec<JoinHanderId>>::new(),
            handler_id: HashMap::<JoinHanderId, DefId>::new(),
            condvars: HashMap::<CondVarId, NodeIndex>::new(),
            entry_node: NodeIndex::new(0),
        }
    }

    pub fn construct(&mut self /*alias_analysis: &'pn RefCell<AliasAnalysis<'pn, 'tcx>>*/) {
        self.construct_func();
        self.construct_lock_with_dfs();
        self.collect_handle();
        self.collect_condvar();
        for (node, caller) in self.callgraph.graph.node_references() {
            if self.tcx.is_mir_available(caller.instance().def_id())
                && format_name(caller.instance().def_id()).starts_with(&self.options.crate_name)
            {
                log::debug!(
                    "visitor function body: {:?}",
                    format_name(caller.instance().def_id())
                );
                self.visitor_function_body(node, caller);
            }
        }
        self.reduce_state();
        //self.reduce_state_from(self.entry_node);
    }

    pub fn visitor_function_body(
        &mut self,
        node: NodeIndex,
        caller: &CallGraphNode<'tcx>,
        //alias_analysis: &'pn RefCell<AliasAnalysis<'pn, 'tcx>>,
    ) {
        let body = self.tcx.optimized_mir(caller.instance().def_id());
        // let body = self.tcx.instance_mir(caller.instance().def);
        // Skip promoted src
        if body.source.promoted.is_some() {
            return;
        }
        let lock_infos = self.lock_info.clone();

        let mut func_body = BodyToPetriNet::new(
            node,
            caller.instance(),
            body,
            self.tcx,
            &self.options,
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
        func_body.translate();
    }

    // Construct Function Start and End Place by callgraph
    pub fn construct_func(&mut self) {
        if let Some((main_func, _)) = self.tcx.entry_fn(()) {
            for node_idx in self.callgraph.graph.node_indices() {
                // println!("{:?}", self.callgraph.graph.node_weight(node_idx).unwrap());
                let func_instance = self.callgraph.graph.node_weight(node_idx).unwrap();
                let func_id = func_instance.instance().def_id();
                let func_name = format_name(func_id);
                if !func_name.contains(&self.options.crate_name) {
                    continue;
                }
                if func_id == main_func {
                    let func_start = Place::new(format!("{}_start", func_name), 1);
                    let func_start_node_id = self.net.add_node(PetriNetNode::P(func_start));
                    let func_end = Place::new_with_no_token(format!("{}_end", func_name));
                    let func_end_node_id = self.net.add_node(PetriNetNode::P(func_end));

                    self.function_counter
                        .insert(func_id, (func_start_node_id, func_end_node_id));
                    // self.function_vec.push(func_start_node_id);
                    self.function_vec.insert(func_id, vec![func_start_node_id]);
                    self.entry_node = func_start_node_id;
                } else {
                    let func_start = Place::new_with_no_token(format!("{}_start", func_name));
                    let func_start_node_id = self.net.add_node(PetriNetNode::P(func_start));
                    let func_end = Place::new_with_no_token(format!("{}_end", func_name));
                    let func_end_node_id = self.net.add_node(PetriNetNode::P(func_end));
                    // println!("function id: {:?}", func_id);
                    self.function_counter
                        .insert(func_id, (func_start_node_id, func_end_node_id));
                    self.function_vec.insert(func_id, vec![func_start_node_id]);
                }
            }
        } else {
            log::debug!("cargo pta need a entry point!");
        }
    }

    // Construct lock for place
    pub fn construct_lock(&mut self, alias_analysis: &RefCell<AliasAnalysis>) {
        let lockguards = self.collect_lockguards();
        // classify the lock point to the same memory location
        // let mut lockguard_relations = FxHashSet::<(LockGuardId, LockGuardId)>::default();
        let mut info = FxHashMap::default();

        for (_, map) in lockguards.clone().into_iter() {
            info.extend(map.clone().into_iter());
            self.lock_info.extend(map.into_iter());
        }

        log::debug!("The count of locks: {:?}", info.keys().count());
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
                    _ => {}
                }
            }
        }

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
        log::debug!("The lock_id_map count?: {:?}", lock_id_map.keys().count());

        for (id, lock_vec) in lock_id_map {
            match &info[&lock_vec[0]].lockguard_ty {
                LockGuardTy::StdMutex(_)
                | LockGuardTy::ParkingLotMutex(_)
                | LockGuardTy::SpinMutex(_) => {
                    let lock_p = Place::new(format!("Mutex_{}", id), 1);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    for lock in lock_vec {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
                _ => {
                    let lock_p = Place::new(format!("RwLock_{}", id), 10);
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
                    let lock_name = format!("Mutex_{}", id);

                    let lock_p = Place::new(lock_name, 1);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    for lock in lock_vec {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
                _ => {
                    let lock_name = format!("RwLock_{}", id);
                    let lock_p = Place::new(lock_name, 10);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    for lock in lock_vec {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
            }
        }
    }

    /// 简化 Petri 网中的状态,通过合并简单路径来减少网络的复杂度
    ///
    /// 具体步骤:
    /// 1. 找到所有入度和出度都≤1的节点作为起始点
    /// 2. 从每个起始点开始,向两个方向(前向和后向)搜索,找到可以合并的路径
    /// 3. 对于每条找到的路径:
    ///    - 确保路径的起点和终点都是 Place 节点
    ///    - 如果路径长度>3,则创建一个新的 Transition 节点来替代中间的节点
    ///    - 保持路径两端的 Place 节点不变,删除中间的所有节点
    /// 4. 最后统一删除所有被标记为需要移除的节点
    ///
    /// 这种简化可以显著减少 Petri 网的大小,同时保持其基本行为特性不变
    pub fn reduce_state(&mut self) {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut all_nodes_to_remove = Vec::new();
        // 找到所有入度和出度都≤1的点
        for node in self.net.node_indices() {
            let in_degree = self.net.edges_directed(node, Direction::Incoming).count();
            let out_degree = self.net.edges_directed(node, Direction::Outgoing).count();

            if in_degree <= 1 && out_degree <= 1 {
                queue.push_back(node);
            }
        }

        while let Some(start) = queue.pop_front() {
            if visited.contains(&start) {
                continue;
            }

            // 从start开始BFS，找到一条链
            let mut chain = vec![start];
            let mut current = start;
            visited.insert(start);

            // 向两个方向遍历
            for direction in &[Direction::Outgoing, Direction::Incoming] {
                current = start;
                loop {
                    let neighbors: Vec<_> =
                        self.net.neighbors_directed(current, *direction).collect();

                    if neighbors.len() != 1 {
                        break;
                    }

                    let next = neighbors[0];
                    let next_in_degree = self.net.edges_directed(next, Direction::Incoming).count();
                    let next_out_degree =
                        self.net.edges_directed(next, Direction::Outgoing).count();

                    if next_in_degree > 1 || next_out_degree > 1 || visited.contains(&next) {
                        break;
                    }

                    visited.insert(next);
                    if *direction == Direction::Outgoing {
                        chain.push(next);
                    } else {
                        chain.insert(0, next);
                    }
                    current = next;
                }
            }

            let chain_len = chain.len();
            // 调整链，确保起始和结束都是Place
            if !chain.is_empty() {
                if let PetriNetNode::T(_) = &self.net[chain[0]] {
                    chain.remove(0);
                }
            }
            if !chain.is_empty() {
                if let PetriNetNode::T(_) = &self.net[chain[chain.len() - 1]] {
                    chain.pop();
                    assert_eq!(chain.len(), chain_len - 1);
                }
            }
            // 检查调整后的链长度是否满足简化条件
            if chain.len() > 3 {
                // 确保chain不为空
                if chain.is_empty() {
                    continue;
                }
                let p1 = chain[0];
                let p2 = chain[chain.len() - 1];

                // 确保p1和p2都是Place
                if let (PetriNetNode::P(_), PetriNetNode::P(_)) = (&self.net[p1], &self.net[p2]) {
                    // 创建新的Transition
                    let new_trans = Transition::new(
                        format!("merged_trans_{}_{}", p1.index(), p2.index()),
                        (0, 0),
                        1,
                    );
                    let new_trans_idx = self.net.add_node(PetriNetNode::T(new_trans));

                    // 添加新边
                    self.net
                        .add_edge(p1, new_trans_idx, PetriNetEdge { label: 1 });
                    self.net
                        .add_edge(new_trans_idx, p2, PetriNetEdge { label: 1 });

                    // 将路径上的节点信息合并成一行输出
                    let path_info = chain[1..chain.len()]
                        .iter()
                        .map(|&node| match &self.net[node] {
                            PetriNetNode::P(place) => format!("P({})", place.name),
                            PetriNetNode::T(transition) => format!("T({})", transition.name),
                        })
                        .collect::<Vec<_>>()
                        .join(" -> ");
                    log::debug!("Path: {}", path_info);
                    // 收集要删除的节点
                    all_nodes_to_remove.extend(chain[1..chain.len() - 1].iter().cloned());
                }
            }
        }
        // 在循环结束后统一删除节点
        if !all_nodes_to_remove.is_empty() {
            // 按索引从大到小排序
            all_nodes_to_remove.sort_by(|a, b| b.index().cmp(&a.index()));
            // 删除节点
            for node in all_nodes_to_remove {
                self.net.remove_node(node);
            }
        }
    }

    /// 分析并简化从起始节点到终止节点的路径，保留与特殊节点相连的路径
    ///
    /// # Arguments
    /// * `start_node` - 起始Place节点
    /// * `end_node` - 终止Place节点
    /// * `special_nodes` - 特殊节点集合，与这些节点相连的路径将被保留
    ///
    /// # 工作流程
    /// 1. 使用DFS收集所有从start_node到end_node的路径
    /// 2. 标记与特殊节点相连的路径为有效路径
    /// 3. 收集需要保留的节点（出现在有效路径中的节点）
    /// 4. 删除仅出现在无效路径中的节点
    pub fn reduce_state_from(
        &mut self,
        start_node: NodeIndex,
        end_node: NodeIndex,
        special_nodes: &[NodeIndex],
    ) {
        // 存储所有从start到end的路径
        let mut all_paths: Vec<Vec<NodeIndex>> = Vec::new();
        // 存储有效路径（与特殊节点相连的路径）
        let mut valid_paths: HashSet<Vec<NodeIndex>> = HashSet::new();
        // 存储当前正在探索的路径
        let mut current_path: Vec<NodeIndex> = vec![start_node];
        // 记录已访问节点，避免简单环路
        let mut visited: HashSet<NodeIndex> = HashSet::new();

        // DFS收集所有路径
        self.collect_paths(
            start_node,
            end_node,
            &mut all_paths,
            &mut current_path,
            &mut visited,
            special_nodes,
            &mut valid_paths,
        );

        // 收集所有需要保留的节点（出现在有效路径中的节点）
        let mut nodes_to_keep: HashSet<NodeIndex> = HashSet::new();
        for path in &valid_paths {
            nodes_to_keep.extend(path.iter().cloned());
        }

        // 收集所有可以删除的节点（出现在无效路径中且不在有效路径中的节点）
        let mut nodes_to_remove: HashSet<NodeIndex> = HashSet::new();
        for path in &all_paths {
            if !valid_paths.contains(path) {
                for &node in path {
                    if !nodes_to_keep.contains(&node) && node != start_node && node != end_node {
                        nodes_to_remove.insert(node);
                    }
                }
            }
        }

        let mut nodes_to_remove: Vec<_> = nodes_to_remove.into_iter().collect();
        nodes_to_remove.sort_by(|a, b| b.index().cmp(&a.index()));
        for node in nodes_to_remove {
            self.net.remove_node(node);
        }
    }

    /// 递归收集从起点到终点的所有路径
    ///
    /// # Arguments
    /// * `current` - 当前正在访问的节点
    /// * `end` - 目标终点节点
    /// * `all_paths` - 存储所有发现的路径
    /// * `current_path` - 当前正在构建的路径
    /// * `visited` - 记录已访问节点，用于避免环路
    /// * `special_nodes` - 特殊节点集合
    /// * `valid_paths` - 存储与特殊节点相连的有效路径
    ///
    /// # 工作流程
    /// 1. 如果到达终点，检查当前路径是否与特殊节点相连
    /// 2. 如果路径有效，添加到valid_paths中
    /// 3. 将当前路径添加到all_paths中
    /// 4. 递归探索所有未访问的邻居节点
    /// 5. 回溯时移除访问标记，允许节点在其他路径中被重复访问
    fn collect_paths(
        &self,
        current: NodeIndex,
        end: NodeIndex,
        all_paths: &mut Vec<Vec<NodeIndex>>,
        current_path: &mut Vec<NodeIndex>,
        visited: &mut HashSet<NodeIndex>,
        special_nodes: &[NodeIndex],
        valid_paths: &mut HashSet<Vec<NodeIndex>>,
    ) {
        if current == end {
            // 检查路径是否与特殊节点相连
            let path_has_special_connection = current_path.iter().any(|&node| {
                self.net
                    .neighbors(node)
                    .any(|neighbor| special_nodes.contains(&neighbor))
            });

            if path_has_special_connection {
                valid_paths.insert(current_path.clone());
            }
            all_paths.push(current_path.clone());
            return;
        }

        visited.insert(current);

        for neighbor in self.net.neighbors_directed(current, Direction::Outgoing) {
            if !visited.contains(&neighbor) {
                current_path.push(neighbor);
                self.collect_paths(
                    neighbor,
                    end,
                    all_paths,
                    current_path,
                    visited,
                    special_nodes,
                    valid_paths,
                );
                current_path.pop();
            }
        }

        visited.remove(&current);
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
                    ApproximateAliasKind::Unlikely => todo!(),
                    ApproximateAliasKind::Unknown => todo!(),
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
                CondVarCollector::new(instance_id, instance, body, self.tcx);
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

    pub fn get_current_mark(&self) -> HashSet<(NodeIndex, usize)> {
        let mut current_mark = HashSet::<(NodeIndex, usize)>::new();
        for node in self.net.node_indices() {
            match &self.net[node] {
                PetriNetNode::P(place) => {
                    if *place.tokens.read().unwrap() > 0 {
                        current_mark.insert((node.clone(), *place.tokens.read().unwrap() as usize));
                    }
                }
                PetriNetNode::T(_) => {
                    debug!("{}", "this error!");
                }
            }
        }
        current_mark
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

    pub fn get_or_insert_node(&mut self, def_id: DefId) -> (NodeIndex, NodeIndex) {
        match self.function_counter.entry(def_id) {
            Entry::Occupied(node) => node.get().to_owned(),
            Entry::Vacant(v) => {
                let func_name = self.tcx.def_path_str(def_id);
                let func_start = Place::new(format!("{}_start", func_name), 0);
                let func_start_node_id = self.net.add_node(PetriNetNode::P(func_start));
                let func_end = Place::new(format!("{}_end", func_name), 0);
                let func_end_node_id = self.net.add_node(PetriNetNode::P(func_end));
                *v.insert((func_start_node_id, func_end_node_id))
            }
        }
    }

    pub fn save_petri_net_to_file(&self) {
        use petgraph::dot::{Config, Dot};
        let pn_dot = Dot::with_attr_getters(
            &self.net,
            &[Config::NodeNoLabel],
            &|_, _| "arrowhead = vee".to_string(),
            &|_, nr| {
                let label = match &nr.1 {
                    PetriNetNode::P(place) => {
                        format!("label = \"{}\", shape = circle", place.name)
                    }
                    PetriNetNode::T(transition) => {
                        format!("label = \"{}\", shape = box", transition.name)
                    }
                };
                label
            },
        );

        let mut file = std::fs::File::create("graph.dot").unwrap();
        let _ = file.write_all(format!("{:?}", pn_dot).as_bytes());
    }
}
