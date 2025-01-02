use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::{Direction, Graph};
use rustc_hir::def_id::DefId;

use std::io::Write;

use crate::options::Options;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::hash::Hasher;

use super::pn::{PetriNetEdge, PetriNetNode};

use serde::Serialize;

#[derive(Debug, Clone)]
pub struct StateEdge {
    pub label: String,
    pub transition: NodeIndex,
    pub weight: u32,
}

impl StateEdge {
    pub fn new(label: String, transition: NodeIndex, weight: u32) -> Self {
        Self {
            label,
            transition,
            weight,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StateNode {
    pub mark: Vec<(usize, usize)>,
    pub node_index: HashSet<NodeIndex>,
}

impl Hash for StateNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.mark.hash(state)
    }
}

impl PartialEq for StateNode {
    fn eq(&self, other: &Self) -> bool {
        self.mark == other.mark
    }
}

impl Eq for StateNode {}

impl StateNode {
    pub fn new(mark: Vec<(usize, usize)>, node_index: HashSet<NodeIndex>) -> Self {
        Self { mark, node_index }
    }
}

// 规范化状态表示
pub fn normalize_state(mark: &HashSet<(NodeIndex, usize)>) -> Vec<(usize, usize)> {
    let mut state: Vec<(usize, usize)> = mark.iter().map(|(n, t)| (n.index(), *t)).collect();
    state.sort();
    state
}

pub fn insert_with_comparison(
    set: &mut HashSet<Vec<(usize, usize)>>,
    value: &Vec<(usize, usize)>,
) -> bool {
    for existing_value in set.iter() {
        if existing_value == value {
            return false;
        }
    }
    set.insert(value.clone());
    return true;
}

#[derive(Debug, Serialize)]
pub struct DeadlockInfo {
    pub function_id: String,
    pub start_state: usize,
    pub deadlock_path: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct StateGraph {
    pub graph: Graph<StateNode, StateEdge>,
    pub initial_net: Box<Graph<PetriNetNode, PetriNetEdge>>,
    pub initial_mark: HashSet<(NodeIndex, usize)>,
    pub deadlock_marks: HashSet<Vec<(usize, usize)>>,
    pub apis_deadlock_marks: HashMap<String, HashSet<Vec<(usize, usize)>>>,
    pub apis_graph: HashMap<String, Box<Graph<StateNode, StateEdge>>>,
    pub function_counter: HashMap<DefId, (NodeIndex, NodeIndex)>,
    pub options: Options,
    pub terminal_states: Vec<(usize, usize)>,
}

impl StateGraph {
    pub fn new(
        initial_net: Graph<PetriNetNode, PetriNetEdge>,
        initial_mark: HashSet<(NodeIndex, usize)>,
        function_counter: HashMap<DefId, (NodeIndex, NodeIndex)>,
        options: Options,
        terminal_states: Vec<(usize, usize)>,
    ) -> Self {
        Self {
            graph: Graph::<StateNode, StateEdge>::new(),
            initial_net: Box::new(initial_net),
            initial_mark,
            deadlock_marks: HashSet::new(),
            apis_deadlock_marks: HashMap::new(),
            apis_graph: HashMap::new(),
            function_counter,
            options,
            terminal_states,
        }
    }

    /// 生成 Petri 网从初始状态可达的所有状态
    ///
    /// 该函数使用广度优先搜索和并行处理的方式来探索所有可达状态。
    /// 对于每个状态，计算其使能的变迁，并行地发生这些变迁以生成新状态，
    /// 如果生成的新状态是唯一的，则将其添到状态图中。
    pub fn generate_states(&mut self) {
        let mut queue = VecDeque::new();
        let mut state_index_map = HashMap::<StateNode, NodeIndex>::new();
        let mut visited_states: HashSet<StateNode> = HashSet::new();
        // 初始状态队列，加入初始网和标识
        queue.push_back((self.initial_net.clone(), self.initial_mark.clone()));

        let initial_state = StateNode::new(
            normalize_state(&self.initial_mark),
            self.initial_mark
                .clone()
                .into_iter()
                .map(|(n, _)| n)
                .collect(),
        );
        let initial_node = self.graph.add_node(initial_state.clone());
        state_index_map.insert(initial_state.clone(), initial_node);

        while let Some((mut current_net, current_mark)) = queue.pop_front() {
            // 获取当前状态下所有使能的变迁
            let enabled_transitions = self.get_enabled_transitions(&mut current_net, &current_mark);

            // 如果没有使能的变迁，将当前状态添加到死锁标识集合中
            if enabled_transitions.is_empty() {
                let current_state_normalized = normalize_state(&current_mark);
                self.deadlock_marks.insert(current_state_normalized.clone());
                continue;
            }
            let current_state_node = StateNode::new(
                normalize_state(&current_mark),
                current_mark.clone().into_iter().map(|(n, _)| n).collect(),
            );
            if visited_states.contains(&current_state_node) {
                continue;
            } else {
                visited_states.insert(current_state_node.clone());
            }

            let current_node = state_index_map.get(&current_state_node).unwrap().clone();

            let new_states: Vec<_> = {
                let mut handles = vec![];

                for transition in enabled_transitions {
                    let current_net = current_net.clone();
                    let current_mark = current_mark.clone();
                    let self_clone = self.clone();

                    let handle = std::thread::spawn(move || {
                        let mut net_clone = current_net.clone();
                        let (new_net, new_mark) =
                            self_clone.fire_transition(&mut net_clone, &current_mark, transition);
                        (transition, new_net, new_mark)
                    });

                    handles.push(handle);
                }

                handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .collect()
            };

            // 处理每个新生成的状态
            for (transition, new_net, new_mark) in new_states {
                let mark_node_index = new_mark.iter().map(|(n, _)| *n).collect();
                let new_state = StateNode::new(normalize_state(&new_mark), mark_node_index);

                if let Some(&existing_node) = state_index_map.get(&new_state) {
                    // 状态已存在，只添加边
                    self.graph.add_edge(
                        current_node.clone(),
                        existing_node,
                        StateEdge::new(format!("{:?}", transition), transition, 1),
                    );
                } else {
                    // 新状态，添加节点和边
                    queue.push_back((new_net.clone(), new_mark.clone()));
                    let new_node = self.graph.add_node(new_state.clone());
                    state_index_map.insert(new_state, new_node);

                    self.graph.add_edge(
                        current_node.clone(),
                        new_node,
                        StateEdge::new(format!("{:?}", transition), transition, 1),
                    );
                }
            }
        }
    }

    pub fn generate_states_with_api(
        &mut self,
        api_name: String,
        api_initial_mark: HashSet<(NodeIndex, usize)>,
    ) {
        let mut queue = VecDeque::new();
        let mut state_index_map = HashMap::<Vec<(usize, usize)>, NodeIndex>::new();
        let mut visited_states = HashSet::new();

        // 初始化状态队列
        queue.push_back((self.initial_net.clone(), api_initial_mark.clone()));
        {
            let initial_state = normalize_state(&api_initial_mark);
            let mark_node_index = api_initial_mark.iter().map(|(n, _)| *n).collect();
            let initial_node = self
                .apis_graph
                .entry(api_name.clone())
                .or_insert(Box::new(Graph::<StateNode, StateEdge>::new()))
                .add_node(StateNode::new(initial_state.clone(), mark_node_index));
            state_index_map.insert(initial_state, initial_node);
        }

        while let Some((mut current_net, current_mark)) = queue.pop_front() {
            let enabled_transitions = self.get_enabled_transitions(&mut current_net, &current_mark);

            if enabled_transitions.is_empty() {
                let current_state_normalized = normalize_state(&current_mark);
                self.apis_deadlock_marks
                    .entry(api_name.clone())
                    .or_insert(HashSet::new())
                    .insert(current_state_normalized);
                continue;
            }

            let current_state = normalize_state(&current_mark);
            if !visited_states.insert(current_state.clone()) {
                continue;
            }

            let current_node = *state_index_map.get(&current_state).unwrap();

            let new_states: Vec<_> = {
                let mut handles = vec![];
                for transition in enabled_transitions {
                    let current_net = current_net.clone();
                    let current_mark = current_mark.clone();
                    let self_clone = self.clone();

                    let handle = std::thread::spawn(move || {
                        let mut net_clone = current_net.clone();
                        let (new_net, new_mark) =
                            self_clone.fire_transition(&mut net_clone, &current_mark, transition);
                        (transition, new_net, new_mark)
                    });
                    handles.push(handle);
                }
                handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .collect()
            };

            for (transition, new_net, new_mark) in new_states {
                let new_state = normalize_state(&new_mark);
                let mark_node_index = new_mark.iter().map(|(n, _)| *n).collect();

                let existing_node = state_index_map.get(&new_state);
                if existing_node.is_none() {
                    queue.push_back((new_net.clone(), new_mark.clone()));
                    let new_node = self
                        .apis_graph
                        .entry(api_name.clone())
                        .or_insert(Box::new(Graph::<StateNode, StateEdge>::new()))
                        .add_node(StateNode::new(new_state.clone(), mark_node_index));

                    state_index_map.insert(new_state, new_node);

                    self.apis_graph
                        .entry(api_name.clone())
                        .or_insert(Box::new(Graph::<StateNode, StateEdge>::new()))
                        .add_edge(
                            current_node,
                            new_node,
                            StateEdge::new(format!("{:?}", transition), transition, 1),
                        );
                } else {
                    self.apis_graph
                        .entry(api_name.clone())
                        .or_insert(Box::new(Graph::<StateNode, StateEdge>::new()))
                        .add_edge(
                            current_node,
                            *existing_node.unwrap(),
                            StateEdge::new(format!("{:?}", transition), transition, 1),
                        );
                }
            }
        }
    }

    #[inline]
    fn set_current_mark(
        &self,
        net: &mut Graph<PetriNetNode, PetriNetEdge>,
        mark: &HashSet<(NodeIndex, usize)>,
    ) {
        // 首先将所有库所的 token 清零
        for node_index in net.node_indices() {
            if let Some(PetriNetNode::P(place)) = net.node_weight(node_index) {
                *place.tokens.write().unwrap() = 0;
            }
        }

        // 直接根据 mark 中的 NodeIndex 设置对应的 token
        for (node_index, token_count) in mark {
            if let Some(PetriNetNode::P(place)) = net.node_weight(*node_index) {
                // let tokens = *place.tokens.write().unwrap();
                {
                    *place.tokens.write().unwrap() = *token_count;
                }
                assert!(
                    *place.tokens.read().unwrap() <= place.capacity,
                    "Token count ({}) exceeds capacity ({}) at node index {}, and token_count is {} ",
                    *place.tokens.read().unwrap(),
                    place.capacity,
                    node_index.index(),
                    token_count
                );
            }
        }
    }

    /// 获取当前标识下所有使能的变迁
    /// 1. 使用 `set_current_mark` 函数设置当前标识
    /// 2. 遍历网络中的每个节点，检查其是否为变迁节点
    /// 3. 对于每个变迁节点，检查其所有输入库所是否有足够的 token
    /// 4. 如果所有输入库所的 token 数量均满足要求，则该变迁为使能状态
    /// 5. 将所有使能的变迁节点索引添加到返回的向量中
    pub fn get_enabled_transitions(
        &self,
        net: &mut Graph<PetriNetNode, PetriNetEdge>,
        mark: &HashSet<(NodeIndex, usize)>,
    ) -> Vec<NodeIndex> {
        let mut sched_transiton = Vec::<NodeIndex>::new();

        // 使用内联函数设置当前标识
        self.set_current_mark(net, mark);

        // 检查变迁使能的逻辑
        for node_index in net.node_indices() {
            match net.node_weight(node_index) {
                Some(PetriNetNode::T(_)) => {
                    let mut enabled = true;
                    for edge in net.edges_directed(node_index, Direction::Incoming) {
                        match net.node_weight(edge.source()).unwrap() {
                            PetriNetNode::P(place) => {
                                if *place.tokens.read().unwrap() < edge.weight().label {
                                    enabled = false;
                                    break;
                                }
                            }
                            _ => {
                                log::error!("The predecessor set of transition is not place");
                            }
                        }
                    }
                    if enabled {
                        sched_transiton.push(node_index);
                    }
                }
                _ => continue,
            }
        }

        sched_transiton
    }

    /// 发生一个变迁并生成新的网络状态
    /// 1. 克隆当前网络创建新图
    /// 2. 根据当前标识设置初始 token
    /// 3. 从变迁的输入库所中减去相应的 token
    /// 4. 向变迁的输出库所中添加相应的 token（考虑容量限制）
    /// 5. 生成并返回新的状态
    pub fn fire_transition(
        &self,
        net: &mut Graph<PetriNetNode, PetriNetEdge>,
        mark: &HashSet<(NodeIndex, usize)>,
        transition: NodeIndex,
    ) -> (
        Box<Graph<PetriNetNode, PetriNetEdge>>,
        HashSet<(NodeIndex, usize)>,
    ) {
        let mut new_net = net.clone(); // 克隆当前网，创建新图
        self.set_current_mark(&mut new_net, mark);
        let mut new_state = HashSet::<(NodeIndex, usize)>::new();
        log::debug!("The transition to fire is: {}", transition.index());

        // 从输入库所中减去token
        log::debug!("sub token to source node!");
        for edge in new_net.edges_directed(transition, Direction::Incoming) {
            match new_net.node_weight(edge.source()).unwrap() {
                PetriNetNode::P(place) => {
                    let mut tokens = place.tokens.write().unwrap();
                    *tokens -= edge.weight().label;
                }
                PetriNetNode::T(_) => {
                    log::error!("{}", "this error!");
                }
            }
        }

        // 将token添加到输出库所中
        log::debug!("add token to target node!");
        for edge in new_net.edges_directed(transition, Direction::Outgoing) {
            let place_node = new_net.node_weight(edge.target()).unwrap();
            match place_node {
                PetriNetNode::P(place) => {
                    let mut tokens = place.tokens.write().unwrap();
                    *tokens += edge.weight().label;
                    if *tokens > place.capacity {
                        *tokens = place.capacity;
                    }
                    assert!(place.capacity > 0);
                }
                PetriNetNode::T(_) => {
                    log::error!("{}", "this error!");
                }
            }
        }

        log::debug!("generate new state!");
        for node in new_net.node_indices() {
            match &new_net[node] {
                PetriNetNode::P(place) => {
                    let tokens = *place.tokens.read().unwrap();
                    if tokens > 0 {
                        // 确保token数量不超过容量限制
                        let final_tokens = tokens.min(place.capacity);
                        new_state.insert((node, final_tokens));
                    }
                }
                PetriNetNode::T(_) => {}
            }
        }

        (Box::new(new_net), new_state) // 返回新图和新状态
    }

    /// 运行死锁检测
    pub fn detect_deadlock(&self) -> String {
        use crate::detect::deadlock::DeadlockDetector;

        let detector = DeadlockDetector::new(self);
        let report = detector.detect();

        format!("{}", report)
    }

    #[allow(dead_code)]
    pub fn dot(&self) -> std::io::Result<()> {
        if self.options.dump_options.dump_state_graph {
            let sg_dot = format!(
                "digraph {{\n{:?}\n}}",
                Dot::with_config(&self.graph, &[Config::GraphContentOnly])
            );

            let mut file = std::fs::File::create(
                self.options
                    .output
                    .as_ref()
                    .unwrap()
                    .join("state_graph.dot"),
            )
            .unwrap();
            let _ = file.write_all(format!("{:?}", sg_dot).as_bytes());
            log::info!(
                "State graph saved to {}",
                self.options
                    .output
                    .as_ref()
                    .unwrap()
                    .join("state_graph.dot")
                    .display()
            );
        }
        Ok(())
    }
}
